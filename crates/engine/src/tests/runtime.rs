use super::element::{engine, run};
use super::in_child;

#[cfg(target_os = "linux")]
#[test]
fn random_values_never_fall_back_to_zeros_when_no_file_can_be_opened() {
    let test = "tests::runtime::random_values_never_fall_back_to_zeros_when_no_file_can_be_opened";
    let child = in_child(test, &["prlimit", "--nofile=64:64"], || {
        let (mut e, sink) = engine();
        let held: Vec<std::fs::File> = std::iter::from_fn(|| std::fs::File::open("/dev/null").ok())
            .take(64)
            .collect();
        assert!(
            std::fs::File::open("/dev/urandom").is_err(),
            "{} fds held",
            held.len()
        );
        let log = run(
            &mut e,
            &sink,
            r#"
            const b = crypto.getRandomValues(new Uint8Array(32));
            L(b.some(x => x !== 0) && crypto.randomUUID() !== crypto.randomUUID() ? "random" : "zeros");
        "#,
        );
        assert_eq!(log, ["random"]);
    });
    if let Some(c) = child {
        assert!(c.status.success(), "{:?}\n{}", c.status, c.stderr);
    }
}
