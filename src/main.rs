#[cfg(feature = "tracing_subscriber_tracy")]
#[global_allocator]
static GLOBAL_ALLOCATOR: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 0);

fn main() -> eyre::Result<()> {
    teamy_studio::main()
}
