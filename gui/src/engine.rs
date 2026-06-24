//! Ponte entre a UI e o domínio `decfec`.
//!
//! Reproduz a orquestração da CLT (`src/main.rs`), porém devolvendo valores em
//! vez de imprimir, e convertendo os erros da lib em texto legível para exibir
//! nos painéis. Nenhum I/O aqui — só strings RON entram e saem.

use std::collections::BTreeSet;

use decfec::fault::{Indicators, Scenario};
use decfec::topology::Network;

/// Resultado de um cálculo DEC/FEC sobre um conjunto de consumidores.
pub struct Report {
    /// Descrição do conjunto (ex.: "a jusante da chave '1'" ou "sistema inteiro").
    pub alvo: String,
    /// Consumidores do conjunto (Cc).
    pub cc: u32,
    /// Indicadores calculados.
    pub ind: Indicators,
}

/// Parseia e **valida** a rede a partir do texto RON.
pub fn load_network(ron: &str) -> Result<Network, String> {
    let net = Network::from_ron(ron).map_err(|e| format!("erro de parse na rede: {e}"))?;
    net.validate().map_err(|e| e.to_string())?;
    Ok(net)
}

/// Serializa a rede em RON legível (export do grafo para texto).
pub fn network_to_ron(net: &Network) -> String {
    to_ron(net)
}

/// Serializa o cenário em RON legível.
pub fn scenario_to_ron(scenario: &Scenario) -> String {
    to_ron(scenario)
}

fn to_ron<T: serde::Serialize>(value: &T) -> String {
    let cfg = ron::ser::PrettyConfig::default();
    ron::ser::to_string_pretty(value, cfg).unwrap_or_else(|e| format!("// erro ao serializar: {e}"))
}

/// Parseia um cenário a partir do texto RON.
pub fn load_scenario(ron: &str) -> Result<Scenario, String> {
    Scenario::from_ron(ron).map_err(|e| format!("erro de parse no cenário: {e}"))
}

/// Simula o cenário sobre a rede e calcula DEC/FEC.
///
/// `switch`: vazio/`None` → sistema inteiro ([`Network::line_indices`]); caso
/// contrário, o conjunto a jusante dessa chave ([`Network::downstream_lines`]).
pub fn run(net: &Network, scenario: &Scenario, switch: Option<&str>) -> Result<Report, String> {
    let res = scenario.simulate(net).map_err(|e| e.to_string())?;

    let (conjunto, alvo): (BTreeSet<usize>, String) = match switch {
        Some(s) if !s.is_empty() => match net.downstream_lines(s) {
            Some(set) => (set, format!("a jusante da chave '{s}'")),
            None => return Err(format!("chave '{s}' não encontrada")),
        },
        _ => (net.line_indices(), "sistema inteiro".to_string()),
    };

    let cc = net.consumers_of(&conjunto);
    let ind = res.indicators(net, &conjunto);
    Ok(Report { alvo, cc, ind })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prova que a UI está fiada corretamente ao domínio: a rede e o cenário de
    // referência embutidos devem reproduzir o gabarito do item (a).
    const REDE: &str = include_str!("../../networks/ref-exercise.ron");
    const CENARIO: &str = include_str!("../../scenarios/item_a.ron");

    #[test]
    fn item_a_bate_gabarito() {
        let net = load_network(REDE).expect("rede de referência deve carregar");
        let scenario = load_scenario(CENARIO).expect("cenário de referência deve carregar");
        let r = run(&net, &scenario, Some("1")).expect("simulação deve rodar");
        assert_eq!(r.cc, 5400, "Cc do alimentador SD1");
        assert!((r.ind.dec_h - 2.33).abs() < 0.01, "DEC = {}", r.ind.dec_h);
        assert!((r.ind.fec - 1.15).abs() < 0.01, "FEC = {}", r.ind.fec);
    }

    #[test]
    fn export_de_rede_volta_a_carregar() {
        // grafo → RON → grafo deve preservar os indicadores (round-trip).
        let net = load_network(REDE).unwrap();
        let net2 = load_network(&network_to_ron(&net)).expect("RON exportado deve recarregar");
        assert_eq!(net.branches.len(), net2.branches.len());

        let scenario = load_scenario(CENARIO).unwrap();
        let scenario2 =
            load_scenario(&scenario_to_ron(&scenario)).expect("cenário exportado deve recarregar");
        let r = run(&net2, &scenario2, Some("1")).unwrap();
        assert_eq!(r.cc, 5400);
        assert!((r.ind.dec_h - 2.33).abs() < 0.01);
    }
}
