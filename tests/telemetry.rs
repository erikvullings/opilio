use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use opilio::{
    ssh::{
        CancellationToken, OpenSsh, ProcessAdapter, ProcessError, ProcessOutput, ProcessRequest,
    },
    telemetry::{
        Metric, PollSchedule, ProviderSnapshot, ProviderState, RemoteTelemetryExecutor,
        RemoteTelemetryOutput, TelemetryCollector, TelemetryHistory,
        nvidia::{MemorySemantics, PlatformIdentity, parse_nvidia_help, parse_nvidia_query},
        system::parse_linux_system,
    },
};

struct FakeRemote {
    outputs: Mutex<VecDeque<RemoteTelemetryOutput>>,
    commands: Mutex<Vec<String>>,
}

impl FakeRemote {
    fn new(outputs: Vec<RemoteTelemetryOutput>) -> Self {
        Self {
            outputs: Mutex::new(outputs.into()),
            commands: Mutex::new(Vec::new()),
        }
    }
}

impl RemoteTelemetryExecutor for FakeRemote {
    fn run(
        &self,
        target: &str,
        shell: &str,
        command: &str,
        _cancellation: &CancellationToken,
    ) -> Result<RemoteTelemetryOutput, String> {
        assert_eq!(target, "spark.local");
        assert_eq!(shell, "/bin/sh");
        self.commands.lock().unwrap().push(command.to_owned());
        Ok(self.outputs.lock().unwrap().pop_front().unwrap())
    }
}

struct FakeSshProcess {
    outputs: Mutex<VecDeque<ProcessOutput>>,
    requests: Mutex<Vec<ProcessRequest>>,
}

impl FakeSshProcess {
    fn new(outputs: Vec<ProcessOutput>) -> Self {
        Self {
            outputs: Mutex::new(outputs.into()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl ProcessAdapter for FakeSshProcess {
    fn find_executable(&self, name: &str) -> Option<PathBuf> {
        (name == "ssh").then(|| PathBuf::from("/system/bin/ssh"))
    }

    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, ProcessError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(self.outputs.lock().unwrap().pop_front().unwrap())
    }

    fn interactive(&self, _request: &ProcessRequest) -> Result<i32, ProcessError> {
        unreachable!("telemetry never opens an interactive SSH process")
    }
}

fn ssh_output(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.as_bytes().to_vec(),
        ..ProcessOutput::default()
    }
}

#[test]
fn parses_realistic_linux_proc_metrics() {
    let metrics =
        parse_linux_system(include_str!("fixtures/system/linux-proc.txt")).expect("valid fixture");

    assert_eq!(metrics.load.one, Metric::Available { value: 0.42 });
    assert_eq!(metrics.load.five, Metric::Available { value: 0.31 });
    assert_eq!(
        metrics.memory.total_bytes,
        Metric::Available {
            value: 135_065_624_576
        }
    );
    assert_eq!(
        metrics.memory.available_bytes,
        Metric::Available {
            value: 33_766_408_192
        }
    );
    assert_eq!(metrics.uptime_seconds, Metric::Available { value: 86_461 });
    assert_eq!(metrics.logical_cpus, Metric::Available { value: 2 });
    assert_eq!(metrics.cpu_busy_percent, Metric::Available { value: 15.0 });
}

#[test]
fn malformed_system_values_are_unavailable_not_zero() {
    let metrics =
        parse_linux_system(include_str!("fixtures/system/malformed.txt")).expect("sections exist");

    assert_eq!(metrics.load.one, Metric::Unavailable);
    assert_eq!(metrics.memory.total_bytes, Metric::Unavailable);
    assert_eq!(metrics.uptime_seconds, Metric::Unavailable);
    assert_eq!(metrics.cpu_busy_percent, Metric::Unavailable);
    assert_eq!(metrics.logical_cpus, Metric::Unavailable);
}

#[test]
fn parses_ordinary_nvidia_gpu_with_dedicated_memory() {
    let supported = parse_nvidia_help(include_str!("fixtures/nvidia/help-query-gpu.txt"));
    let snapshot = parse_nvidia_query(
        &supported,
        include_str!("fixtures/nvidia/ordinary.csv"),
        &PlatformIdentity {
            product_name: Some("Precision 7960 Tower".into()),
            device_tree_model: None,
        },
    )
    .expect("valid fixture");

    assert_eq!(snapshot.state, ProviderState::Available);
    assert_eq!(snapshot.gpus.len(), 1);
    let gpu = &snapshot.gpus[0];
    assert_eq!(gpu.name.as_deref(), Some("NVIDIA RTX 6000 Ada Generation"));
    assert_eq!(gpu.utilization_percent, Metric::Available { value: 87.0 });
    assert_eq!(gpu.memory.semantics, MemorySemantics::Dedicated);
    assert_eq!(
        gpu.memory.total_bytes,
        Metric::Available {
            value: 51_527_024_640
        }
    );
    assert_eq!(
        gpu.memory.used_bytes,
        Metric::Available {
            value: 45_948_600_320
        }
    );
}

#[test]
fn detects_dgx_spark_gb10_and_never_reports_conventional_vram() {
    let supported = parse_nvidia_help(include_str!("fixtures/nvidia/help-query-gpu.txt"));
    let snapshot = parse_nvidia_query(
        &supported,
        include_str!("fixtures/nvidia/spark.csv"),
        &PlatformIdentity {
            product_name: Some("NVIDIA DGX Spark".into()),
            device_tree_model: Some("NVIDIA GB10".into()),
        },
    )
    .expect("valid fixture");

    let memory = &snapshot.gpus[0].memory;
    assert_eq!(memory.semantics, MemorySemantics::Unified);
    assert_eq!(memory.total_bytes, Metric::Unsupported);
    assert_eq!(memory.used_bytes, Metric::Unsupported);
}

#[test]
fn unsupported_and_malformed_nvidia_fields_are_explicit() {
    let supported = parse_nvidia_help("\"index\"\n\"name\"\n\"utilization.gpu\"\n");
    let snapshot = parse_nvidia_query(
        &supported,
        "0, NVIDIA T4, not-supported\n",
        &PlatformIdentity::default(),
    )
    .expect("row remains usable");
    let gpu = &snapshot.gpus[0];

    assert_eq!(gpu.utilization_percent, Metric::Unavailable);
    assert_eq!(gpu.temperature_celsius, Metric::Unsupported);
    assert_eq!(gpu.power_watts, Metric::Unsupported);
    assert_eq!(gpu.memory.total_bytes, Metric::Unsupported);
}

#[test]
fn malformed_nvidia_rows_do_not_create_fabricated_devices() {
    let supported = parse_nvidia_help("\"index\"\n\"name\"\n");
    let error = parse_nvidia_query(
        &supported,
        "this row has too few columns\n",
        &PlatformIdentity::default(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("columns"));
}

#[test]
fn bounded_history_and_poll_schedule_are_rendering_independent() {
    let mut history = TelemetryHistory::new(2);
    history.push(SystemTime::UNIX_EPOCH, 1.0);
    history.push(SystemTime::UNIX_EPOCH + Duration::from_secs(1), 2.0);
    history.push(SystemTime::UNIX_EPOCH + Duration::from_secs(2), 3.0);
    assert_eq!(
        history
            .iter()
            .map(|sample| sample.value)
            .collect::<Vec<_>>(),
        [2.0, 3.0]
    );

    let now = SystemTime::UNIX_EPOCH;
    let mut fast = PollSchedule::new(Duration::from_secs(2), now);
    let mut slow = PollSchedule::new(Duration::from_secs(10), now);
    let cancellation = CancellationToken::new();
    let calls = Mutex::new(Vec::new());

    assert!(fast.run_if_due(now, &cancellation, || calls.lock().unwrap().push("fast")));
    assert!(slow.run_if_due(now, &cancellation, || calls.lock().unwrap().push("slow")));
    assert!(!fast.run_if_due(now + Duration::from_secs(1), &cancellation, || {}));
    assert!(
        fast.run_if_due(now + Duration::from_secs(2), &cancellation, || calls
            .lock()
            .unwrap()
            .push("fast"))
    );
    cancellation.cancel();
    assert!(
        !slow.run_if_due(now + Duration::from_secs(10), &cancellation, || {
            calls.lock().unwrap().push("cancelled")
        })
    );
    assert_eq!(*calls.lock().unwrap(), ["fast", "slow", "fast"]);
}

#[test]
fn provider_collects_system_and_discovers_nvidia_fields_over_ssh_seam() {
    let fake = FakeRemote::new(vec![
        RemoteTelemetryOutput::success(include_str!("fixtures/system/linux-proc.txt")),
        RemoteTelemetryOutput::success(include_str!("fixtures/nvidia/help-query-gpu.txt")),
        RemoteTelemetryOutput::success("NVIDIA DGX Spark\nNVIDIA GB10\n"),
        RemoteTelemetryOutput::success(include_str!("fixtures/nvidia/spark.csv")),
    ]);
    let collector = TelemetryCollector::new(&fake);

    let snapshot = collector.collect(
        "spark.local",
        "/bin/sh",
        opilio::domain::Telemetry::Nvidia,
        &CancellationToken::new(),
    );

    assert!(matches!(
        snapshot.system,
        ProviderSnapshot::Available { .. }
    ));
    let ProviderSnapshot::Available { data } = snapshot.nvidia.unwrap() else {
        panic!("NVIDIA metrics should be available");
    };
    assert_eq!(data.gpus[0].memory.semantics, MemorySemantics::Unified);
    assert_eq!(data.gpus[0].memory.total_bytes, Metric::Unsupported);
    let commands = fake.commands.lock().unwrap();
    assert_eq!(commands.len(), 4);
    assert!(commands[1].contains("--help-query-gpu"));
    assert!(commands[3].contains("memory.total"));
}

#[test]
fn missing_nvidia_tool_is_explicitly_unsupported() {
    let fake = FakeRemote::new(vec![
        RemoteTelemetryOutput::success(include_str!("fixtures/system/linux-proc.txt")),
        RemoteTelemetryOutput {
            exit_code: Some(127),
            stdout: String::new(),
            stderr: "nvidia-smi: not found".into(),
        },
    ]);
    let collector = TelemetryCollector::new(&fake);

    let snapshot = collector.collect(
        "spark.local",
        "/bin/sh",
        opilio::domain::Telemetry::Nvidia,
        &CancellationToken::new(),
    );

    assert_eq!(snapshot.nvidia, Some(ProviderSnapshot::Unsupported));
}

#[test]
fn cancellation_prevents_remote_telemetry_commands() {
    let fake = FakeRemote::new(Vec::new());
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let snapshot = TelemetryCollector::new(&fake).collect(
        "spark.local",
        "/bin/sh",
        opilio::domain::Telemetry::Nvidia,
        &cancellation,
    );

    assert!(matches!(
        snapshot.system,
        ProviderSnapshot::Unavailable { .. }
    ));
    assert!(fake.commands.lock().unwrap().is_empty());
}

#[test]
fn telemetry_can_reuse_the_openssh_control_master_adapter() {
    let process = Arc::new(FakeSshProcess::new(vec![
        ssh_output(""),
        ssh_output(include_str!("fixtures/system/linux-proc.txt")),
        ssh_output(include_str!("fixtures/nvidia/help-query-gpu.txt")),
        ssh_output("NVIDIA DGX Spark\nNVIDIA GB10\n"),
        ssh_output(include_str!("fixtures/nvidia/spark.csv")),
        ssh_output(""),
    ]));
    let ssh = OpenSsh::discover(process.clone()).unwrap();
    let master = ssh
        .start_control_master("spark.local", Path::new("target/telemetry-%C"))
        .unwrap();

    let snapshot = TelemetryCollector::new(&master).collect(
        "ignored-because-master-is-bound",
        "/bin/sh",
        opilio::domain::Telemetry::Nvidia,
        &CancellationToken::new(),
    );
    master.close().unwrap();

    assert!(matches!(
        snapshot.system,
        ProviderSnapshot::Available { .. }
    ));
    let requests = process.requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    for request in &requests[1..5] {
        assert!(
            request
                .arguments()
                .iter()
                .any(|argument| argument == "ControlPath=target/telemetry-%C")
        );
    }
}
