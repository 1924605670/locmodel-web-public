use std::future::Future;
use std::sync::LazyLock;
use tokio::runtime::Runtime;

static RT: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("locmodel-io")
        .build()
        .expect("无法创建后台运行时")
});

pub fn block_on<F>(future: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    RT.block_on(future)
}
