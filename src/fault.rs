//! Faltas como uma **simulação a eventos discretos**.
//!
//! Em vez de descrever o "resultado" de cada falta, descreve-se o que acontece
//! na rede ao longo do tempo (uma linha do tempo de [`Event`]s) e o motor
//! **deriva** quem ficou sem energia, calculando conectividade a cada instante.
//! Isso compõe naturalmente faltas simultâneas, rede já reconfigurada quando a
//! próxima falta chega, e blocos interrompidos mais de uma vez.
//!
//! Os tempos são sempre em **minutos**.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::topology::{Element, Network, State};

/// Limiar do PRODIST: interrupções com duração **< 3 min** são momentâneas e
/// não entram nos indicadores.
pub const MOMENTARY_LIMIT_MIN: f64 = 3.0;

/// O que um evento faz com um ramo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// O ramo entra em curto: deixa de conduzir até o reparo.
    Fault,
    /// O ramo (antes em falta) volta a conduzir.
    Repair,
    /// A chave abre.
    Open,
    /// A chave fecha.
    Close,
}

/// Um evento na linha do tempo: em `at_min`, aplica `action` ao ramo `branch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub at_min: f64,
    pub branch: String,
    pub action: Action,
}

/// Roteiro: a linha do tempo de eventos a simular.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub events: Vec<Event>,
}

/// Resultado da simulação: por índice de ramo-linha, as durações (min) de cada
/// interrupção sofrida (ainda **sem** o filtro dos 3 min — ele é aplicado no
/// cálculo dos indicadores).
#[derive(Debug, Clone, Default)]
pub struct SimResult {
    pub interruptions: HashMap<usize, Vec<f64>>,
}

/// Indicadores de um conjunto de consumidores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Indicators {
    /// DEC em **horas**.
    pub dec_h: f64,
    /// FEC (nº de interrupções por consumidor).
    pub fec: f64,
}

impl SimResult {
    /// Calcula DEC/FEC de um conjunto (índices de ramos-linha), aplicando o
    /// filtro dos 3 min (interrupções momentâneas são descartadas).
    pub fn indicators(&self, net: &Network, conjunto: &BTreeSet<usize>) -> Indicators {
        let cc = net.consumers_of(conjunto);
        if cc == 0 {
            return Indicators {
                dec_h: 0.0,
                fec: 0.0,
            };
        }
        let mut dec_num = 0.0; // consumidor·min
        let mut fec_num = 0.0; // consumidor
        for &i in conjunto {
            let consumers = net.branches[i].consumers() as f64;
            let durations = self.interruptions.get(&i).map(Vec::as_slice).unwrap_or(&[]);
            for &d in durations {
                if d >= MOMENTARY_LIMIT_MIN {
                    dec_num += consumers * d;
                    fec_num += consumers;
                }
            }
        }
        Indicators {
            dec_h: dec_num / 60.0 / cc as f64,
            fec: fec_num / cc as f64,
        }
    }
}

/// Erros da simulação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    UnknownBranch(String),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::UnknownBranch(id) => write!(f, "evento referencia ramo inexistente: '{id}'"),
        }
    }
}

impl std::error::Error for SimError {}

impl Scenario {
    /// Lê um cenário de uma string no formato RON.
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// Simula a linha do tempo e devolve as interrupções de cada bloco.
    ///
    /// O estado da rede é constante entre dois instantes de evento; em cada
    /// fase calcula-se a energização e marca-se quais blocos estão sem energia.
    /// Sequências contíguas sem energia viram **uma** interrupção. O roteiro
    /// deve terminar com tudo restaurado (faltas reparadas).
    pub fn simulate(&self, net: &Network) -> Result<SimResult, SimError> {
        let n = net.branches.len();

        // Estado inicial (config normal): NA aberta, NF fechada; nada em falta.
        let mut is_open = vec![false; n];
        for (i, b) in net.branches.iter().enumerate() {
            if let Element::Switch { normal } = b.element {
                is_open[i] = normal == State::Open;
            }
        }
        let mut is_failed = vec![false; n];

        // Instantes de evento, ordenados e únicos.
        let mut times: Vec<f64> = self.events.iter().map(|e| e.at_min).collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        if times.len() < 2 {
            // Sem intervalo a integrar: nenhuma interrupção fechada.
            return Ok(SimResult::default());
        }

        let lines = net.line_indices();
        // out_phase[linha] = vetor de bool, um por fase [times[k], times[k+1]).
        let mut out_phase: HashMap<usize, Vec<bool>> = lines
            .iter()
            .map(|&i| (i, Vec::with_capacity(times.len() - 1)))
            .collect();

        for &t in &times[..times.len() - 1] {
            // Aplica todos os eventos deste instante (antes da fase k).
            for e in self.events.iter().filter(|e| e.at_min == t) {
                let idx = net
                    .branch_index(&e.branch)
                    .ok_or_else(|| SimError::UnknownBranch(e.branch.clone()))?;
                match e.action {
                    Action::Fault => is_failed[idx] = true,
                    Action::Repair => is_failed[idx] = false,
                    Action::Open => is_open[idx] = true,
                    Action::Close => is_open[idx] = false,
                }
            }

            // Energização nesta fase: um ramo conduz se não está em falta e não
            // está aberto. Um bloco é servido se sua linha não está em falta e
            // tem ao menos um extremo energizado.
            let energ = net.energized(|i, _b| !is_failed[i] && !is_open[i]);
            for &li in &lines {
                let b = &net.branches[li];
                let served = !is_failed[li]
                    && (energ.contains(b.from.as_str()) || energ.contains(b.to.as_str()));
                out_phase.get_mut(&li).unwrap().push(!served);
            }
        }

        // Converte as fases sem energia em durações de interrupção por bloco.
        let last = *times.last().unwrap();
        let mut interruptions: HashMap<usize, Vec<f64>> = HashMap::new();
        for (&li, phases) in &out_phase {
            let mut start: Option<f64> = None;
            for (k, &out) in phases.iter().enumerate() {
                match (out, start) {
                    (true, None) => start = Some(times[k]),
                    (false, Some(s)) => {
                        interruptions.entry(li).or_default().push(times[k] - s);
                        start = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = start {
                interruptions.entry(li).or_default().push(last - s);
            }
        }

        Ok(SimResult { interruptions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Alimentador radial: S --[br]-- n0 --(A:100)-- n1 --[sw_mid]-- n2
    //                       --(B:200)-- n3 --[sw_end]-- n4 --(C:300)-- n5 --[NA]-- S2
    const NET: &str = r#"
        Network(
            buses: [
                (id: "S",  kind: Substation),
                (id: "n0", kind: Junction),
                (id: "n1", kind: Junction),
                (id: "n2", kind: Junction),
                (id: "n3", kind: Junction),
                (id: "n4", kind: Junction),
                (id: "n5", kind: Junction),
                (id: "S2", kind: Substation),
            ],
            branches: [
                (id: Some("br"),     from: "S",  to: "n0", element: Switch(normal: Closed)),
                (id: Some("A"),      from: "n0", to: "n1", element: Line(consumers: 100)),
                (id: Some("sw_mid"), from: "n1", to: "n2", element: Switch(normal: Closed)),
                (id: Some("B"),      from: "n2", to: "n3", element: Line(consumers: 200)),
                (id: Some("sw_end"), from: "n3", to: "n4", element: Switch(normal: Closed)),
                (id: Some("C"),      from: "n4", to: "n5", element: Line(consumers: 300)),
                (id: Some("NA"),     from: "n5", to: "S2", element: Switch(normal: Open)),
            ],
        )
    "#;

    // Falta em B: proteção (br) derruba tudo; em 2 min isola B e religa o
    // montante (A); em 30 min transfere C pela NA; em 120 min repara B.
    fn scenario() -> Scenario {
        let ev = |at_min: f64, branch: &str, action: Action| Event {
            at_min,
            branch: branch.into(),
            action,
        };
        Scenario {
            events: vec![
                ev(0.0, "B", Action::Fault),
                ev(0.0, "br", Action::Open),
                ev(2.0, "sw_mid", Action::Open),
                ev(2.0, "sw_end", Action::Open),
                ev(2.0, "br", Action::Close),
                ev(30.0, "NA", Action::Close),
                ev(120.0, "B", Action::Repair),
            ],
        }
    }

    fn net() -> Network {
        Network::from_ron(NET).unwrap()
    }

    #[test]
    fn outage_durations_per_block() {
        let net = net();
        let res = scenario().simulate(&net).unwrap();
        let dur = |id: &str| {
            res.interruptions
                .get(&net.branch_index(id).unwrap())
                .cloned()
        };
        assert_eq!(dur("A"), Some(vec![2.0])); // montante: 0→2 min
        assert_eq!(dur("B"), Some(vec![120.0])); // faltado: 0→reparo
        assert_eq!(dur("C"), Some(vec![30.0])); // transferido: 0→30 min
    }

    #[test]
    fn dec_fec_whole_feeder_drops_momentary() {
        let net = net();
        let res = scenario().simulate(&net).unwrap();
        let ind = res.indicators(&net, &net.line_indices());
        // A (2 min) é descartado. DEC = (200*120 + 300*30)/60/600.
        assert!((ind.dec_h - (200.0 * 120.0 + 300.0 * 30.0) / 60.0 / 600.0).abs() < 1e-9);
        // FEC = (200 + 300)/600 (A não conta).
        assert!((ind.fec - 500.0 / 600.0).abs() < 1e-9);
    }

    #[test]
    fn dec_fec_restricted_to_downstream_conjunto() {
        let net = net();
        let res = scenario().simulate(&net).unwrap();
        let conjunto = net.downstream_lines("sw_end").unwrap(); // só o bloco C
        let ind = res.indicators(&net, &conjunto);
        assert!((ind.dec_h - 300.0 * 30.0 / 60.0 / 300.0).abs() < 1e-9); // 0,5 h
        assert!((ind.fec - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_branch_is_error() {
        let net = net();
        let bad = Scenario {
            events: vec![
                Event {
                    at_min: 0.0,
                    branch: "naoexiste".into(),
                    action: Action::Fault,
                },
                Event {
                    at_min: 1.0,
                    branch: "br".into(),
                    action: Action::Open,
                },
            ],
        };
        assert_eq!(
            bad.simulate(&net).unwrap_err(),
            SimError::UnknownBranch("naoexiste".into())
        );
    }
}
