use tracing::Level;

fn main() -> eyre::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .try_init();
    teamy_studio::main()
}
