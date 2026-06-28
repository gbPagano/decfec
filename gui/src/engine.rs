//! Ponte entre a UI e o domínio `decfec`.
//!
//! Reproduz a orquestração da CLT (`src/main.rs`), porém devolvendo valores em
//! vez de imprimir, e convertendo os erros da lib em texto legível para exibir
//! nos painéis. Nenhum I/O aqui — só strings RON entram e saem.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use decfec::fault::{Indicators, MOMENTARY_LIMIT_MIN, OutageInterval, Scenario};
use decfec::topology::Network;

/// Resultado de um cálculo DEC/FEC sobre um conjunto de consumidores.
pub struct Report {
    /// Descrição do conjunto (ex.: "a jusante da chave '1'" ou "sistema inteiro").
    pub alvo: String,
    /// Consumidores do conjunto (Cc).
    pub cc: u32,
    /// Indicadores calculados.
    pub ind: Indicators,
    /// Memória de cálculo por ramo do conjunto.
    pub lines: Vec<ReportLine>,
}

/// Parcela de cálculo de um ramo do conjunto selecionado.
pub struct ReportLine {
    pub label: String,
    pub consumers: u32,
    pub counted: Vec<OutageInterval>,
    pub discarded: Vec<OutageInterval>,
}

/// Parseia e **valida** a rede a partir do texto RON.
#[cfg(test)]
pub fn load_network(ron: &str) -> Result<Network, String> {
    let net = Network::from_ron(ron).map_err(|e| format!("erro de parse na rede: {e}"))?;
    net.validate().map_err(|e| e.to_string())?;
    Ok(net)
}

/// Serializa a rede em RON legível (export do grafo para texto).
#[cfg(test)]
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
    let mut lines = Vec::new();
    for &i in &conjunto {
        let branch = &net.branches[i];
        let consumers = branch.consumers();
        let intervals = res.intervals.get(&i).cloned().unwrap_or_default();
        let counted: Vec<OutageInterval> = intervals
            .iter()
            .copied()
            .filter(|interval| interval.duration_min() >= MOMENTARY_LIMIT_MIN)
            .collect();
        let discarded: Vec<OutageInterval> = intervals
            .iter()
            .copied()
            .filter(|interval| interval.duration_min() < MOMENTARY_LIMIT_MIN)
            .collect();
        lines.push(ReportLine {
            label: branch.label(),
            consumers,
            counted,
            discarded,
        });
    }

    Ok(Report {
        alvo,
        cc,
        ind,
        lines,
    })
}

pub fn calculation_preview_text(report: &Report) -> String {
    let mut text = String::new();
    let (terms, discarded) = calculation_terms(report);
    writeln!(text, "Memória de cálculo DEC/FEC").unwrap();
    writeln!(text).unwrap();
    writeln!(text, "Conjunto: {}", report.alvo).unwrap();
    writeln!(text, "Cc = {} consumidores", report.cc).unwrap();
    writeln!(text).unwrap();

    writeln!(text, "Interrupções consideradas").unwrap();
    if terms.is_empty() {
        writeln!(text, "nenhuma interrupção válida").unwrap();
    } else {
        for term in &terms {
            writeln!(
                text,
                "- {}: C = {}; início = {} min; duração = {} min",
                term.branch,
                term.consumers,
                fmt_num(term.start_min),
                fmt_num(term.duration_min)
            )
            .unwrap();
        }
    }
    if !discarded.is_empty() {
        writeln!(text, "Descartadas: {}", discarded.join("; ")).unwrap();
    }
    writeln!(text).unwrap();

    let dec_terms = terms
        .iter()
        .map(|term| format!("{} · {}/60", term.consumers, fmt_num(term.duration_min)))
        .collect::<Vec<_>>();
    let dec_expr = if dec_terms.is_empty() {
        "0".to_string()
    } else {
        join_preview_terms(&dec_terms)
    };
    write_fraction_preview(
        &mut text,
        "DEC",
        &dec_expr,
        report.cc,
        &format!("{:.3} h", report.ind.dec_h),
    );

    let fec_terms = terms
        .iter()
        .map(|term| term.consumers.to_string())
        .collect::<Vec<_>>();
    let fec_expr = if fec_terms.is_empty() {
        "0".to_string()
    } else {
        join_preview_terms(&fec_terms)
    };
    write_fraction_preview(
        &mut text,
        "FEC",
        &fec_expr,
        report.cc,
        &format!("{:.3}", report.ind.fec),
    );

    text
}

fn calculation_terms(report: &Report) -> (Vec<CalcTerm>, Vec<String>) {
    let mut terms = Vec::new();
    let mut discarded = Vec::new();
    for line in &report.lines {
        for &interval in &line.counted {
            if line.consumers == 0 {
                continue;
            }
            terms.push(CalcTerm {
                branch: line.label.clone(),
                consumers: line.consumers,
                start_min: interval.start_min,
                duration_min: interval.duration_min(),
            });
        }
        if !line.discarded.is_empty() {
            discarded.push(format!(
                "{}: {} min",
                line.label,
                fmt_interval_list(&line.discarded)
            ));
        }
    }
    (terms, discarded)
}

fn write_fraction_preview(
    text: &mut String,
    label: &str,
    numerator: &str,
    denominator: u32,
    result: &str,
) {
    let bar_len = numerator
        .lines()
        .map(str::len)
        .max()
        .unwrap_or(1)
        .max(denominator.to_string().len());
    writeln!(text, "{label}").unwrap();
    writeln!(text, "{label} = {numerator}").unwrap();
    writeln!(
        text,
        "{}   = {result}",
        "─".repeat(bar_len + label.len() + 3)
    )
    .unwrap();
    writeln!(text, "{}{}", " ".repeat(label.len() + 3), denominator).unwrap();
    writeln!(text).unwrap();
}

fn join_preview_terms(terms: &[String]) -> String {
    const TERMS_PER_LINE: usize = 4;
    terms
        .chunks(TERMS_PER_LINE)
        .map(|chunk| chunk.join(" + "))
        .collect::<Vec<_>>()
        .join(" +\n      ")
}

struct CalcTerm {
    branch: String,
    consumers: u32,
    start_min: f64,
    duration_min: f64,
}

fn fmt_interval_list(values: &[OutageInterval]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            values
                .iter()
                .map(|interval| format!(
                    "início {}, duração {}",
                    fmt_num(interval.start_min),
                    fmt_num(interval.duration_min())
                ))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn fmt_num(value: f64) -> String {
    if (value.fract()).abs() < 1e-9 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prova que a UI está fiada corretamente ao domínio: a rede e o cenário de
    // referência embutidos devem simular eventos sobre barras.
    const REDE: &str = include_str!("../../networks/ref-exercise.ron");
    const CENARIO: &str = include_str!("../../scenarios/item_a.ron");

    #[test]
    fn cenario_embutido_roda() {
        let net = load_network(REDE).expect("rede de referência deve carregar");
        let scenario = load_scenario(CENARIO).expect("cenário de referência deve carregar");
        let r = run(&net, &scenario, Some("2")).expect("simulação deve rodar");
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
        let r = run(&net2, &scenario2, Some("2")).unwrap();
        assert_eq!(r.cc, 5400);
        assert!((r.ind.dec_h - 2.33).abs() < 0.01);
    }

    #[test]
    fn memoria_de_calculo_usa_formato_de_resolucao_manual() {
        let net = load_network(REDE).unwrap();
        let scenario = load_scenario(CENARIO).unwrap();
        let r = run(&net, &scenario, Some("2")).unwrap();

        let text = calculation_preview_text(&r);

        assert!(text.contains("Interrupções consideradas"));
        assert!(text.contains("- "));
        assert!(!text.contains("(F1)"));
        assert!(text.contains("início ="));
        assert!(text.contains("duração ="));
        assert!(text.contains("DEC ="));
        assert!(text.contains("FEC ="));
        assert!(text.contains("·"));
        assert!(text.contains("5400"));
        assert!(!text.contains("+ 0 ·"));
        assert!(!text.contains("\n0 ·"));
        assert!(!text.contains("Eventos simulados"));
    }
}
