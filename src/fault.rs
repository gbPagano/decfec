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

use crate::topology::{BusKind, Network, State};

/// Limiar do PRODIST: interrupções com duração **< 3 min** são momentâneas e
/// não entram nos indicadores.
pub const MOMENTARY_LIMIT_MIN: f64 = 3.0;

/// O que um evento faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Um ramo entra em curto: deixa de servir consumidores até o reparo.
    Fault,
    /// O ramo antes em falta volta a servir.
    Repair,
    /// A chave abre.
    Open,
    /// A chave fecha.
    Close,
}

/// Um evento na linha do tempo: em `at_min`, aplica `action` ao alvo em
/// `branch`. Para `Fault`/`Repair`, o alvo é um ramo; para `Open`/`Close`, uma
/// chave modelada como nó. O nome do campo permanece `branch` para manter o RON
/// curto nos cenários existentes.
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
/// interrupção sofrida (ainda **sem** o filtro dos 3 min - ele é aplicado no
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
        let mut dec_num = 0.0; // consumidor.min
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
    UnknownSwitch(String),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::UnknownBranch(id) => write!(f, "evento referencia ramo inexistente: '{id}'"),
            SimError::UnknownSwitch(id) => write!(f, "evento referencia chave inexistente: '{id}'"),
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
        let branch_count = net.branches.len();
        let bus_count = net.buses.len();

        // Estado inicial (config normal): NA aberta, NF fechada; nada em falta.
        let mut is_open = vec![false; bus_count];
        for (i, b) in net.buses.iter().enumerate() {
            if let BusKind::Switch { normal } = b.kind {
                is_open[i] = normal == State::Open;
            }
        }
        let mut is_failed = vec![false; branch_count];

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
                match e.action {
                    Action::Fault => {
                        let idx = net
                            .branch_index(&e.branch)
                            .ok_or_else(|| SimError::UnknownBranch(e.branch.clone()))?;
                        is_failed[idx] = true;
                    }
                    Action::Repair => {
                        let idx = net
                            .branch_index(&e.branch)
                            .ok_or_else(|| SimError::UnknownBranch(e.branch.clone()))?;
                        is_failed[idx] = false;
                    }
                    Action::Open => {
                        let idx = net
                            .switch_index(&e.branch)
                            .ok_or_else(|| SimError::UnknownSwitch(e.branch.clone()))?;
                        is_open[idx] = true;
                    }
                    Action::Close => {
                        let idx = net
                            .switch_index(&e.branch)
                            .ok_or_else(|| SimError::UnknownSwitch(e.branch.clone()))?;
                        is_open[idx] = false;
                    }
                }
            }

            // Energização nesta fase: um ramo conduz se não está em falta, e um
            // nó conduz se não é uma chave aberta.
            let energ = net.energized(|i, _| !is_failed[i], |i, _| !is_open[i]);
            for &li in &lines {
                let served = !is_failed[li] && net.line_served(li, &energ);
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

    // Alimentador radial: S -- br -- A(100) -- sw_mid -- B(200) -- sw_end
    // -- C(300) -- NA -- S2. Chaves são nós; consumidores ficam nos ramos.
    const NET: &str = r#"
        Network(
            buses: [
                (id: "S",      kind: Substation),
                (id: "br",     kind: Switch(normal: Closed)),
                (id: "sw_mid", kind: Switch(normal: Closed)),
                (id: "sw_end", kind: Switch(normal: Closed)),
                (id: "NA",     kind: Switch(normal: Open)),
                (id: "S2",     kind: Substation),
            ],
            branches: [
                (id: Some("feed"), nodes: ["S", "br"], element: Line(consumers: 0)),
                (id: Some("A"),    nodes: ["br", "sw_mid"], element: Line(consumers: 100)),
                (id: Some("B"),    nodes: ["sw_mid", "sw_end"], element: Line(consumers: 200)),
                (id: Some("C"),    nodes: ["sw_end", "NA"], element: Line(consumers: 300)),
                (id: Some("tie"),  nodes: ["NA", "S2"], element: Line(consumers: 0)),
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
        assert_eq!(dur("A"), Some(vec![2.0])); // montante: 0->2 min
        assert_eq!(dur("B"), Some(vec![120.0])); // faltado: 0->reparo
        assert_eq!(dur("C"), Some(vec![30.0])); // transferido: 0->30 min
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
    fn open_switch_can_deenergize_multiterminal_branch_until_tie_closes() {
        const NET: &str = r#"
            Network(
                buses: [
                    (id: "S",   kind: Substation),
                    (id: "3",   kind: Switch(normal: Closed)),
                    (id: "4",   kind: Switch(normal: Closed)),
                    (id: "5",   kind: Switch(normal: Closed)),
                    (id: "NA1", kind: Switch(normal: Open)),
                    (id: "S2",  kind: Substation),
                ],
                branches: [
                    (id: Some("feed"), nodes: ["S", "3"], element: Line(consumers: 0)),
                    (id: Some("bloco_700"), nodes: ["3", "4", "5", "NA1"], element: Line(consumers: 700)),
                    (id: Some("tie"), nodes: ["NA1", "S2"], element: Line(consumers: 0)),
                ],
            )
        "#;
        let net = Network::from_ron(NET).unwrap();
        net.validate().unwrap();
        let scenario = Scenario {
            events: vec![
                Event {
                    at_min: 0.0,
                    branch: "3".into(),
                    action: Action::Open,
                },
                Event {
                    at_min: 20.0,
                    branch: "NA1".into(),
                    action: Action::Close,
                },
            ],
        };
        let res = scenario.simulate(&net).unwrap();
        assert_eq!(
            res.interruptions
                .get(&net.branch_index("bloco_700").unwrap())
                .cloned(),
            Some(vec![20.0])
        );
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
