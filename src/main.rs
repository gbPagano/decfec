use std::process::ExitCode;

use decfec::topology::Network;

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "networks/exemplo.ron".to_string());

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("erro ao ler '{path}': {e}");
            return ExitCode::FAILURE;
        }
    };

    let net = match Network::from_ron(&text) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("erro de parse em '{path}': {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = net.validate() {
        eprint!("{e}");
        return ExitCode::FAILURE;
    }

    println!("Rede '{path}'");
    println!("  barramentos: {}", net.buses.len());
    println!("  subestações: {:?}", net.sources());
    println!("  ramos:       {}", net.branches.len());
    println!("  consumidores (Cc): {}", net.total_consumers());

    ExitCode::SUCCESS
}
