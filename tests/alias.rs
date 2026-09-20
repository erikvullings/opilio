use std::num::NonZeroUsize;

use opilio::{alias::expand_alias, config::Config, domain::Operation};

#[test]
fn aliases_expand_to_one_closed_operation_with_fixed_target_and_options() {
    let config = Config::from_yaml(
        r#"
devices:
  alpha:
    ssh: alpha
aliases:
  boot:
    operation: on
    target: alpha
    parallel: 3
    wait: true
  inspect:
    operation: status
    target: all
"#,
    )
    .unwrap();

    let boot = expand_alias(&config, "boot").unwrap();
    assert_eq!(boot.operation, Operation::On);
    assert_eq!(boot.target, "alpha");
    assert_eq!(boot.parallelism, NonZeroUsize::new(3).unwrap());
    assert!(boot.wait);
    assert!(!boot.force);

    let inspect = expand_alias(&config, "inspect").unwrap();
    assert_eq!(inspect.operation, Operation::Status);
    assert_eq!(inspect.parallelism, NonZeroUsize::new(4).unwrap());
}

#[test]
fn disruptive_aliases_default_to_sequential_execution() {
    let config = Config::from_yaml(
        r#"
devices:
  alpha:
    ssh: alpha
aliases:
  stop:
    operation: off
    target: all
"#,
    )
    .unwrap();

    assert_eq!(
        expand_alias(&config, "stop").unwrap().parallelism,
        NonZeroUsize::MIN
    );
}

#[test]
fn workflow_shapes_and_inapplicable_options_are_rejected() {
    let workflow = r#"
devices:
  alpha:
    ssh: alpha
aliases:
  workflow:
    operation: status
    target: all
    steps: [status, status]
"#;
    assert!(
        Config::from_yaml(workflow)
            .unwrap_err()
            .to_string()
            .contains("steps")
    );

    let forced_status = workflow.replace("    steps: [status, status]", "    force: true");
    assert!(
        Config::from_yaml(&forced_status)
            .unwrap_err()
            .to_string()
            .contains("does not support `force`")
    );
}
