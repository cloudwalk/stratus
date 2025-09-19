use stratus_macros::FakeEnum;

fn test_func<T: Default>() -> T {
    T::default()
}

trait FakeEnum {
    fn fake(arm: &str) -> Self;
}

#[derive(FakeEnum)]
#[fake_enum(generate = "crate::test_func")]
#[allow(dead_code)]
pub enum TestEnum {
    First(String),
    Second((u8, String)),
}

#[test]
fn test_process_enum() {
    TestEnum::fake("First");
    TestEnum::fake("Second");
}
