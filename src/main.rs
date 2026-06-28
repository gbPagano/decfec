use std::collections::BTreeSet;
use std::process::ExitCode;

use decfec::fault::Scenario;
use decfec::topology::Network;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .unwrap_or_else(|| "networks/exemplo.ron".to_string());

    let net = match load_net(&path) {
        Ok(n) => n,
        Err(code) => return code,
    };

    match args.next().as_deref() {
        None => summary(&path, &net),
        Some("downstream") => inspect_downstream(&net, args.next()),
        Some("dec-fec") => report_dec_fec(&net, args.next(), args.next()),
        Some("dic-fic-dmic") => report_dic_fic_dmic(&net, args.next(), args.next()),
        Some(other) => {
            eprintln!("subcomando desconhecido: '{other}'");
            eprintln!(
                "uso: decfec <rede.ron> [downstream <chave> | dec-fec <cenário.ron> [chave] | dic-fic-dmic <cenário.ron> <barramento>]"
            );
            ExitCode::FAILURE
        }
    }
}

/// Lê, parseia e valida a rede, ou imprime o erro e devolve o código de saída.
fn load_net(path: &str) -> Result<Network, ExitCode> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("erro ao ler '{path}': {e}");
        ExitCode::FAILURE
    })?;
    let net = Network::from_ron(&text).map_err(|e| {
        eprintln!("erro de parse em '{path}': {e}");
        ExitCode::FAILURE
    })?;
    net.validate().map_err(|e| {
        eprint!("{e}");
        ExitCode::FAILURE
    })?;
    Ok(net)
}

fn summary(path: &str, net: &Network) -> ExitCode {
    println!("Rede '{path}'");
    println!("  barramentos: {}", net.buses.len());
    println!("  subestações: {:?}", net.sources());
    println!("  ramos:       {}", net.branches.len());
    println!("  consumidores (Cc): {}", net.total_consumers());
    ExitCode::SUCCESS
}

fn inspect_downstream(net: &Network, switch: Option<String>) -> ExitCode {
    let Some(switch) = switch else {
        eprintln!("uso: decfec <rede> downstream <chave>");
        return ExitCode::FAILURE;
    };
    match net.downstream_lines(&switch) {
        Some(lines) => {
            let ids: Vec<&str> = lines
                .iter()
                .filter_map(|&i| net.branches[i].id.as_deref())
                .collect();
            println!(
                "a jusante de '{switch}': {} consumidores",
                net.consumers_of(&lines)
            );
            println!("  blocos: {ids:?}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("chave '{switch}' não encontrada");
            ExitCode::FAILURE
        }
    }
}

fn report_dec_fec(
    net: &Network,
    scenario_path: Option<String>,
    switch: Option<String>,
) -> ExitCode {
    let Some(scenario_path) = scenario_path else {
        eprintln!("uso: decfec <rede> dec-fec <cenário.ron> [chave]");
        return ExitCode::FAILURE;
    };

    let scenario = match load_scenario(&scenario_path) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let res = match scenario.simulate(net) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    // Conjunto: a jusante da chave dada, ou o sistema inteiro.
    let (conjunto, alvo): (BTreeSet<usize>, String) = match &switch {
        Some(c) => match net.downstream_lines(c) {
            Some(set) => (set, format!("a jusante da chave '{c}'")),
            None => {
                eprintln!("chave '{c}' não encontrada");
                return ExitCode::FAILURE;
            }
        },
        None => (net.line_indices(), "sistema inteiro".to_string()),
    };

    let cc = net.consumers_of(&conjunto);
    let ind = res.indicators(net, &conjunto);

    println!("Conjunto: {alvo} — Cc = {cc} consumidores");
    println!("  DEC = {:.3} h  ({:.1} min)", ind.dec_h, ind.dec_h * 60.0);
    println!("  FEC = {:.3} interrupções", ind.fec);
    ExitCode::SUCCESS
}

fn report_dic_fic_dmic(
    net: &Network,
    scenario_path: Option<String>,
    bus: Option<String>,
) -> ExitCode {
    let Some(scenario_path) = scenario_path else {
        eprintln!("uso: decfec <rede> dic-fic-dmic <cenário.ron> <barramento>");
        return ExitCode::FAILURE;
    };
    let Some(bus) = bus else {
        eprintln!("uso: decfec <rede> dic-fic-dmic <cenário.ron> <barramento>");
        return ExitCode::FAILURE;
    };

    let scenario = match load_scenario(&scenario_path) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let res = match scenario.simulate(net) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let line = match net.point_load_line(&bus) {
        Ok(line) => line,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let ind = res.individual_indicators_for_line(line);

    println!(
        "Ponto consumidor: {bus} (ramo '{}')",
        net.branches[line].label()
    );
    println!("  DIC = {:.3} h  ({:.1} min)", ind.dic_h, ind.dic_h * 60.0);
    println!("  FIC = {:.3} interrupções", ind.fic as f64);
    println!(
        "  DMIC = {:.3} h  ({:.1} min)",
        ind.dmic_h,
        ind.dmic_h * 60.0
    );
    ExitCode::SUCCESS
}

fn load_scenario(path: &str) -> Result<Scenario, ExitCode> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("erro ao ler '{path}': {e}");
        ExitCode::FAILURE
    })?;
    Scenario::from_ron(&text).map_err(|e| {
        eprintln!("erro de parse em '{path}': {e}");
        ExitCode::FAILURE
    })
}
