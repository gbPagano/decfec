//! Topologia da rede de distribuição como um grafo.
//!
//! Modelo: os **barramentos** (nós) são declarados explicitamente em
//! [`Network::buses`]; cada um tem um `id` único e um [`BusKind`]. Os **ramos**
//! (arestas) ligam dois barramentos *já declarados* e são de dois tipos:
//!
//! - [`Element::Line`]   — trecho de rede que carrega um bloco de consumidores
//!   (o número em itálico do diagrama). Sempre conduz.
//! - [`Element::Switch`] — uma chave manobrável, sem carga. `Open` representa
//!   uma chave NA (*Normalmente Aberta* / tie); `Closed`, uma chave NF
//!   (*Normalmente Fechada* / seccionadora).
//!
//! Essa separação (carga nas linhas, manobra nas chaves) deixa o cálculo de
//! conectividade trivial e é o que o motor de faltas vai usar depois.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

/// Tipo de um barramento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusKind {
    /// Subestação — injeta energia (uma fonte do sistema).
    Substation,
    /// Ponto de conexão sem fonte (junção, derivação, ponto de chave).
    Junction,
}

/// Barramento (nó) da rede.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bus {
    pub id: String,
    pub kind: BusKind,
}

impl Bus {
    pub fn is_source(&self) -> bool {
        self.kind == BusKind::Substation
    }
}

/// Estado de uma chave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    /// Fechada — conduz.
    Closed,
    /// Aberta — não conduz.
    Open,
}

/// O que um ramo representa fisicamente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Element {
    /// Trecho com um bloco de `consumers` unidades consumidoras. Sempre conduz.
    Line { consumers: u32 },
    /// Chave manobrável (sem carga). `normal` é o estado em operação normal.
    Switch { normal: State },
}

/// Ramo do grafo: liga `from` a `to` por um [`Element`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    /// Identificador opcional (ex.: `"1"`, `"22"`, `"NA3"`). Obrigatório na
    /// prática para chaves, pois é por ele que as manobras as referenciam.
    #[serde(default)]
    pub id: Option<String>,
    pub from: String,
    pub to: String,
    pub element: Element,
}

impl Branch {
    /// Consumidores neste ramo (0 para chaves).
    pub fn consumers(&self) -> u32 {
        match self.element {
            Element::Line { consumers } => consumers,
            Element::Switch { .. } => 0,
        }
    }

    /// `true` se for uma chave NA (chave normalmente aberta / tie).
    pub fn is_tie(&self) -> bool {
        matches!(
            self.element,
            Element::Switch {
                normal: State::Open
            }
        )
    }

    /// `true` se for uma chave (NA ou NF).
    pub fn is_switch(&self) -> bool {
        matches!(self.element, Element::Switch { .. })
    }

    /// Rótulo legível para mensagens (id, ou os extremos se não houver id).
    pub fn label(&self) -> String {
        match &self.id {
            Some(id) => id.clone(),
            None => format!("{}–{}", self.from, self.to),
        }
    }
}

/// Rede de distribuição (grafo de barramentos e ramos).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub buses: Vec<Bus>,
    pub branches: Vec<Branch>,
}

impl Network {
    /// Lê uma rede de uma string no formato RON.
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// Ids de todos os barramentos declarados.
    pub fn bus_ids(&self) -> BTreeSet<&str> {
        self.buses.iter().map(|b| b.id.as_str()).collect()
    }

    /// Ids dos barramentos que são fontes (subestações).
    pub fn sources(&self) -> Vec<&str> {
        self.buses
            .iter()
            .filter(|b| b.is_source())
            .map(|b| b.id.as_str())
            .collect()
    }

    /// Total de unidades consumidoras da rede (o `Cc` do sistema inteiro).
    pub fn total_consumers(&self) -> u32 {
        self.branches.iter().map(Branch::consumers).sum()
    }

    /// Mapa barramento -> índices dos ramos incidentes (lista de adjacência).
    pub fn adjacency(&self) -> HashMap<&str, Vec<usize>> {
        let mut adj: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, b) in self.branches.iter().enumerate() {
            adj.entry(b.from.as_str()).or_default().push(i);
            adj.entry(b.to.as_str()).or_default().push(i);
        }
        adj
    }

    /// Validações estruturais. Retorna a lista de problemas encontrados.
    pub fn validate(&self) -> Result<(), TopologyError> {
        let mut problems = Vec::new();

        // Ids de barramento únicos.
        let mut seen_buses = BTreeSet::new();
        for bus in &self.buses {
            if !seen_buses.insert(bus.id.as_str()) {
                problems.push(format!("id de barramento duplicado: '{}'", bus.id));
            }
        }

        if !self.buses.iter().any(Bus::is_source) {
            problems.push("a rede não tem nenhuma subestação (kind: Substation)".into());
        }

        // Mundo fechado: todo extremo de ramo precisa ser um barramento declarado.
        let mut seen_branch_ids = BTreeSet::new();
        for b in &self.branches {
            for end in [&b.from, &b.to] {
                if !seen_buses.contains(end.as_str()) {
                    problems.push(format!(
                        "ramo '{}' referencia barramento inexistente '{end}'",
                        b.label()
                    ));
                }
            }
            if b.from == b.to {
                problems.push(format!(
                    "ramo '{}' liga o barramento '{}' a si mesmo",
                    b.label(),
                    b.from
                ));
            }
            if let Some(id) = &b.id
                && !seen_branch_ids.insert(id.as_str())
            {
                problems.push(format!("id de ramo duplicado: '{id}'"));
            }
        }

        // Conectividade considerando TODOS os ramos (chaves abertas inclusas):
        // a rede física deve ser uma peça só.
        if let Some(start) = self.buses.first() {
            let reachable = self.reachable_from(&start.id, |_| true);
            for bus in &self.buses {
                if !reachable.contains(bus.id.as_str()) {
                    problems.push(format!(
                        "barramento '{}' está desconectado do restante da rede",
                        bus.id
                    ));
                }
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(TopologyError(problems))
        }
    }

    /// Barramentos alcançáveis a partir de `start` percorrendo apenas os ramos
    /// para os quais `conduz(branch)` é verdadeiro. Base para o motor de faltas.
    pub fn reachable_from<'a>(
        &'a self,
        start: &'a str,
        conduz: impl Fn(&Branch) -> bool,
    ) -> BTreeSet<&'a str> {
        let adj = self.adjacency();
        let mut visited = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(bus) = stack.pop() {
            if !visited.insert(bus) {
                continue;
            }
            for &i in adj.get(bus).map(Vec::as_slice).unwrap_or(&[]) {
                let b = &self.branches[i];
                if !conduz(b) {
                    continue;
                }
                let other = if b.from == bus {
                    b.to.as_str()
                } else {
                    b.from.as_str()
                };
                stack.push(other);
            }
        }
        visited
    }
}

/// Conjunto de problemas estruturais encontrados na validação.
#[derive(Debug, Clone)]
pub struct TopologyError(pub Vec<String>);

impl fmt::Display for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "topologia inválida ({} problema(s)):", self.0.len())?;
        for p in &self.0 {
            writeln!(f, "  - {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for TopologyError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        Network(
            buses: [
                (id: "SD_A", kind: Substation),
                (id: "a1", kind: Junction),
                (id: "a2", kind: Junction),
                (id: "a3", kind: Junction),
                (id: "a4", kind: Junction),
                (id: "SD_B", kind: Substation),
                (id: "b1", kind: Junction),
                (id: "b2", kind: Junction),
            ],
            branches: [
                (id: Some("dA"), from: "SD_A", to: "a1", element: Switch(normal: Closed)),
                (from: "a1", to: "a2", element: Line(consumers: 500)),
                (id: Some("sa"), from: "a2", to: "a3", element: Switch(normal: Closed)),
                (from: "a3", to: "a4", element: Line(consumers: 300)),

                (id: Some("dB"), from: "SD_B", to: "b1", element: Switch(normal: Closed)),
                (from: "b1", to: "b2", element: Line(consumers: 400)),

                (id: Some("NA"), from: "a4", to: "b2", element: Switch(normal: Open)),
            ],
        )
    "#;

    fn sample() -> Network {
        Network::from_ron(SAMPLE).expect("RON válido")
    }

    #[test]
    fn parses_and_counts() {
        let net = sample();
        assert_eq!(net.total_consumers(), 1200);
        assert_eq!(net.buses.len(), 8);
        assert_eq!(net.sources(), ["SD_A", "SD_B"]);
        net.validate().expect("rede válida");
    }

    #[test]
    fn tie_is_recognized() {
        let net = sample();
        let na = net.branches.iter().find(|b| b.label() == "NA").unwrap();
        assert!(na.is_tie());
    }

    #[test]
    fn reachability_respects_open_switches() {
        let net = sample();
        // Em operação normal (NA aberta), partindo de SD_A não se alcança o lado B.
        let energizado = net.reachable_from("SD_A", |b| match b.element {
            Element::Switch { normal } => normal == State::Closed,
            Element::Line { .. } => true,
        });
        assert!(energizado.contains("a4"));
        assert!(!energizado.contains("b2"));
    }

    #[test]
    fn rejects_reference_to_undeclared_bus() {
        let mut net = sample();
        net.branches.push(Branch {
            id: Some("solta".into()),
            from: "a4".into(),
            to: "fantasma".into(), // não declarado em `buses`
            element: Element::Line { consumers: 10 },
        });
        assert!(net.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_bus_id() {
        let mut net = sample();
        net.buses.push(Bus {
            id: "a1".into(),
            kind: BusKind::Junction,
        });
        assert!(net.validate().is_err());
    }
}
