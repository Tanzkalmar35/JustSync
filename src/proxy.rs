use std::process::Stdio;
use tokio::process::Command;

pub async fn start_proxy(
    target_cmd: String,
    target_args: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new(target_cmd)
        .args(target_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn child process");

    let mut child_stdin = child.stdin.take().expect("Failed to open stdin");
    let mut child_stdout = child.stdout.take().expect("Failed to open stdout");
    let mut child_stderr = child.stderr.take().expect("Failed to open stderr");

    let mut parent_stdin = tokio::io::stdin();
    let mut parent_stdout = tokio::io::stdout();
    let mut parent_stderr = tokio::io::stderr();

    // tokio::io::copy creates a highly optimized loop that reads from A and writes to B until EOF is reached.

    // Task A: Parent Stdin -> Child Stdin
    let stdin_task = tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut parent_stdin, &mut child_stdin).await {
            eprintln!("Error copying stdin: {}", e);
        }
    });

    // Task B: Child Stdout -> Parent Stdout
    let stdout_task = tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut child_stdout, &mut parent_stdout).await {
            eprintln!("Error copying stdout: {}", e);
        }
    });

    // Task C: Child Stderr -> Parent Stderr
    let stderr_task = tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut child_stderr, &mut parent_stderr).await {
            eprintln!("Error copying stderr: {}", e);
        }
    });

    let status = child.wait().await?;

    let _ = tokio::join!(stdin_task, stdout_task, stderr_task);

    eprintln!("Proxy: Child process exited with {}", status);
    Ok(())
}
