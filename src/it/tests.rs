use {
    crate::it::{test_error::TestError, testrun::TestRun},
    std::{future::Future, rc::Rc},
};

macro_rules! testcase {
    () => {
        pub struct Test;

        impl crate::it::tests::TestCase for Test {
            fn name(&self) -> &'static str {
                module_path!().strip_prefix("jay::it::tests::").unwrap()
            }

            fn run(
                &self,
                testrun: std::rc::Rc<crate::it::testrun::TestRun>,
            ) -> Box<dyn std::future::Future<Output = Result<(), TestError>>> {
                Box::new(test(testrun))
            }
        }
    };
}

macro_rules! tassert {
    ($cond:expr) => {
        if !$cond {
            bail!(
                "Assert `{}` failed ({}:{})",
                stringify!($cond),
                file!(),
                line!()
            );
        }
    };
}

mod t0001_shm_formats;

pub trait TestCase {
    fn name(&self) -> &'static str;
    fn run(&self, testrun: Rc<TestRun>) -> Box<dyn Future<Output = Result<(), TestError>>>;
}

pub fn tests() -> Vec<&'static dyn TestCase> {
    vec![&t0001_shm_formats::Test]
}