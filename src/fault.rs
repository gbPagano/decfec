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

use crate::topology::{BusKind, Network, PointLoadError, State};

/// Limiar do PRODIST: interrupções com duração **< 3 min** são momentâneas e
/// não entram nos indicadores.
pub const MOMENTARY_LIMIT_MIN: f64 = 3.0;

/// O que um evento faz em uma barra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// A barra entra em curto: deixa de conduzir até o reparo.
    Fault,
    /// A barra antes em falta volta a conduzir.
    Repair,
    /// A chave abre.
    Open,
    /// A chave fecha.
    Close,
}

/// Um evento na linha do tempo: em `at_min`, aplica `action` à barra `bus`.
/// `Fault`/`Repair` servem para qualquer barra; `Open`/`Close`, para chaves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub at_min: f64,
    pub bus: String,
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
    pub intervals: HashMap<usize, Vec<OutageInterval>>,
}

/// Intervalo contínuo em que um ramo ficou desenergizado.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutageInterval {
    pub start_min: f64,
    pub end_min: f64,
}

impl OutageInterval {
    pub fn duration_min(&self) -> f64 {
        self.end_min - self.start_min
    }
}

/// Indicadores de um conjunto de consumidores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Indicators {
    /// DEC em **horas**.
    pub dec_h: f64,
    /// FEC (nº de interrupções por consumidor).
    pub fec: f64,
}

/// Indicadores individuais de continuidade de uma unidade consumidora/bloco.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndividualIndicators {
    /// DIC em **horas**: duração total das interrupções válidas.
    pub dic_h: f64,
    /// FIC: quantidade de interrupções válidas.
    pub fic: u32,
    /// DMIC em **horas**: maior interrupção contínua válida.
    pub dmic_h: f64,
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

    /// Calcula DIC/FIC/DMIC para o ramo-linha informado, aplicando o filtro dos
    /// 3 min (interrupções momentâneas são descartadas).
    pub fn individual_indicators_for_line(&self, branch_idx: usize) -> IndividualIndicators {
        let durations = self
            .interruptions
            .get(&branch_idx)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let counted = durations
            .iter()
            .copied()
            .filter(|d| *d >= MOMENTARY_LIMIT_MIN);
        let mut total_min = 0.0;
        let mut max_min = 0.0;
        let mut count = 0;
        for duration in counted {
            total_min += duration;
            max_min = f64::max(max_min, duration);
            count += 1;
        }

        IndividualIndicators {
            dic_h: total_min / 60.0,
            fic: count,
            dmic_h: max_min / 60.0,
        }
    }

    /// Calcula DIC/FIC/DMIC para o ponto consumidor indicado por um barramento.
    ///
    /// O barramento deve pertencer a exatamente um ramo-linha com consumidores;
    /// isso modela um ponto de carga como `X` ou `Y` no diagrama.
    pub fn individual_indicators_for_bus(
        &self,
        net: &Network,
        bus: &str,
    ) -> Result<IndividualIndicators, PointLoadError> {
        let line = net.point_load_line(bus)?;
        Ok(self.individual_indicators_for_line(line))
    }
}

/// Erros da simulação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    UnknownBus(String),
    UnknownSwitch(String),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::UnknownBus(id) => write!(f, "evento referencia barra inexistente: '{id}'"),
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
        let bus_count = net.buses.len();

        // Estado inicial (config normal): NA aberta, NF fechada; nada em falta.
        let mut is_open = vec![false; bus_count];
        for (i, b) in net.buses.iter().enumerate() {
            if let BusKind::Switch { normal } = b.kind {
                is_open[i] = normal == State::Open;
            }
        }
        let mut is_failed = vec![false; bus_count];

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
                            .bus_index(&e.bus)
                            .ok_or_else(|| SimError::UnknownBus(e.bus.clone()))?;
                        is_failed[idx] = true;
                    }
                    Action::Repair => {
                        let idx = net
                            .bus_index(&e.bus)
                            .ok_or_else(|| SimError::UnknownBus(e.bus.clone()))?;
                        is_failed[idx] = false;
                    }
                    Action::Open => {
                        let idx = net
                            .switch_index(&e.bus)
                            .ok_or_else(|| SimError::UnknownSwitch(e.bus.clone()))?;
                        is_open[idx] = true;
                    }
                    Action::Close => {
                        let idx = net
                            .switch_index(&e.bus)
                            .ok_or_else(|| SimError::UnknownSwitch(e.bus.clone()))?;
                        is_open[idx] = false;
                    }
                }
            }

            // Energização nesta fase: ramos sempre conduzem; uma barra conduz
            // se não é uma chave aberta e não está em falta.
            let energ = net.energized(|_, _| true, |i, _| !is_open[i] && !is_failed[i]);
            for &li in &lines {
                let served = net.line_served(li, &energ);
                out_phase.get_mut(&li).unwrap().push(!served);
            }
        }

        // Converte as fases sem energia em durações de interrupção por bloco.
        let last = *times.last().unwrap();
        let mut interruptions: HashMap<usize, Vec<f64>> = HashMap::new();
        let mut intervals: HashMap<usize, Vec<OutageInterval>> = HashMap::new();
        for (&li, phases) in &out_phase {
            let mut start: Option<f64> = None;
            for (k, &out) in phases.iter().enumerate() {
                match (out, start) {
                    (true, None) => start = Some(times[k]),
                    (false, Some(s)) => {
                        let interval = OutageInterval {
                            start_min: s,
                            end_min: times[k],
                        };
                        interruptions
                            .entry(li)
                            .or_default()
                            .push(interval.duration_min());
                        intervals.entry(li).or_default().push(interval);
                        start = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = start {
                let interval = OutageInterval {
                    start_min: s,
                    end_min: last,
                };
                interruptions
                    .entry(li)
                    .or_default()
                    .push(interval.duration_min());
                intervals.entry(li).or_default().push(interval);
            }
        }

        Ok(SimResult {
            interruptions,
            intervals,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Alimentador radial: S -- br -- A(100) -- sw_mid -- B(200) -- F
    // -- C(300) -- NA -- S2. F é o ponto de falta: normalmente conduz como
    // uma junção; durante Fault, deixa de conduzir.
    const NET: &str = r#"
        Network(
            buses: [
                (id: "S",      kind: Substation),
                (id: "br",     kind: Switch(normal: Closed)),
                (id: "sw_mid", kind: Switch(normal: Closed)),
                (id: "F",      kind: Junction),
                (id: "NA",     kind: Switch(normal: Open)),
                (id: "S2",     kind: Substation),
            ],
            branches: [
                (id: Some("feed"), nodes: ["S", "br"], element: Line(consumers: 0)),
                (id: Some("A"),    nodes: ["br", "sw_mid"], element: Line(consumers: 100)),
                (id: Some("B"),    nodes: ["sw_mid", "F"], element: Line(consumers: 200)),
                (id: Some("C"),    nodes: ["F", "NA"], element: Line(consumers: 300)),
                (id: Some("tie"),  nodes: ["NA", "S2"], element: Line(consumers: 0)),
            ],
        )
    "#;

    // Falta em F: proteção (br) derruba tudo; em 2 min isola F e religa o
    // montante (A); em 30 min transfere C pela NA; em 120 min repara B.
    fn scenario() -> Scenario {
        let ev = |at_min: f64, bus: &str, action: Action| Event {
            at_min,
            bus: bus.into(),
            action,
        };
        Scenario {
            events: vec![
                ev(0.0, "F", Action::Fault),
                ev(0.0, "br", Action::Open),
                ev(2.0, "sw_mid", Action::Open),
                ev(2.0, "br", Action::Close),
                ev(30.0, "NA", Action::Close),
                ev(120.0, "F", Action::Repair),
                ev(120.0, "sw_mid", Action::Close),
                ev(121.0, "br", Action::Close),
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

        let intervals = res.intervals.get(&net.branch_index("B").unwrap()).unwrap();
        assert_eq!(
            intervals,
            &[OutageInterval {
                start_min: 0.0,
                end_min: 120.0
            }]
        );
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
        let conjunto = net.downstream_lines("sw_mid").unwrap(); // blocos B e C
        let ind = res.indicators(&net, &conjunto);
        assert!((ind.dec_h - (200.0 * 120.0 + 300.0 * 30.0) / 60.0 / 500.0).abs() < 1e-9);
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
                    bus: "3".into(),
                    action: Action::Open,
                },
                Event {
                    at_min: 20.0,
                    bus: "NA1".into(),
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
    fn unknown_bus_is_error() {
        let net = net();
        let bad = Scenario {
            events: vec![
                Event {
                    at_min: 0.0,
                    bus: "naoexiste".into(),
                    action: Action::Fault,
                },
                Event {
                    at_min: 1.0,
                    bus: "br".into(),
                    action: Action::Open,
                },
            ],
        };
        assert_eq!(
            bad.simulate(&net).unwrap_err(),
            SimError::UnknownBus("naoexiste".into())
        );
    }
}
