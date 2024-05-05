mod hello_triangle;

#[tokio::main]
async fn main() {
    use hello_triangle::main as run;
    run().await;
}