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

/// Simula o cenário sobre a rede e calcula DEC/FEC.
///
/// `switch`: vazio/`None` → sistema inteiro ([`Network::line_indices`]); caso
/// contrário, o conjunto a jusante dessa chave ([`Network::downstream_lines`]).
pub fn run(net: &Network, scenario_ron: &str, switch: Option<&str>) -> Result<Report, String> {
    let scenario =
        Scenario::from_ron(scenario_ron).map_err(|e| format!("erro de parse no cenário: {e}"))?;
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
        let r = run(&net, CENARIO, Some("1")).expect("simulação deve rodar");
        assert_eq!(r.cc, 5400, "Cc do alimentador SD1");
        assert!((r.ind.dec_h - 2.33).abs() < 0.01, "DEC = {}", r.ind.dec_h);
        assert!((r.ind.fec - 1.15).abs() < 0.01, "FEC = {}", r.ind.fec);
    }
}
