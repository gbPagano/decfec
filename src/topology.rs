//! Topologia da rede de distribuição como um grafo com ramos multi-terminais.
//!
//! Modelo: os **barramentos** (nós) são declarados explicitamente em
//! [`Network::buses`]. Uma chave é modelada como um nó (`BusKind::Switch`), com
//! estado normal aberto/fechado. Os **ramos** ([`Branch`]) conectam dois ou mais
//! nós e carregam consumidores.
//!
//! Isso evita criar barramentos artificiais dos dois lados de cada chave. Um
//! bloco do diagrama pode ser declarado como um único ramo que interliga todas
//! as chaves que encostam nele, por exemplo `nodes: ["3", "4", "5", "NA1"]`.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

/// Estado de uma chave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    /// Fechada - conduz.
    Closed,
    /// Aberta - não conduz.
    Open,
}

/// Tipo de um barramento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusKind {
    /// Subestação - injeta energia (uma fonte do sistema).
    Substation,
    /// Ponto de conexão sem fonte nem manobra.
    Junction,
    /// Chave manobrável. `Open` representa NA/tie; `Closed`, NF/seccionadora.
    Switch { normal: State },
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

    pub fn is_switch(&self) -> bool {
        matches!(self.kind, BusKind::Switch { .. })
    }

    pub fn is_tie(&self) -> bool {
        matches!(
            self.kind,
            BusKind::Switch {
                normal: State::Open
            }
        )
    }

    pub fn conducts_normally(&self) -> bool {
        match self.kind {
            BusKind::Substation | BusKind::Junction => true,
            BusKind::Switch { normal } => normal == State::Closed,
        }
    }
}

/// O que um ramo representa fisicamente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Element {
    /// Trecho/bloco com `consumers` unidades consumidoras.
    Line { consumers: u32 },
}

/// Ramo do grafo: interliga dois ou mais nós por um [`Element`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    /// Identificador opcional (ex.: `"tr_3_4"`, `"bloco_700"`). Obrigatório na
    /// prática para faltas, pois é por ele que os eventos as referenciam.
    #[serde(default)]
    pub id: Option<String>,
    /// Nós terminais do ramo. Pode ter mais de dois nós.
    pub nodes: Vec<String>,
    pub element: Element,
}

impl Branch {
    /// Consumidores neste ramo.
    pub fn consumers(&self) -> u32 {
        match self.element {
            Element::Line { consumers } => consumers,
        }
    }

    /// Rótulo legível para mensagens (id, ou os terminais se não houver id).
    pub fn label(&self) -> String {
        match &self.id {
            Some(id) => id.clone(),
            None => self.nodes.join("-"),
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

    /// Ids das chaves declaradas como nós.
    pub fn switches(&self) -> Vec<&str> {
        self.buses
            .iter()
            .filter(|b| b.is_switch())
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
            for node in &b.nodes {
                adj.entry(node.as_str()).or_default().push(i);
            }
        }
        adj
    }

    /// Validações estruturais. Retorna a lista de problemas encontrados.
    pub fn validate(&self) -> Result<(), TopologyError> {
        let mut problems = Vec::new();

        let mut seen_buses = BTreeSet::new();
        for bus in &self.buses {
            if !seen_buses.insert(bus.id.as_str()) {
                problems.push(format!("id de barramento duplicado: '{}'", bus.id));
            }
        }

        if !self.buses.iter().any(Bus::is_source) {
            problems.push("a rede não tem nenhuma subestação (kind: Substation)".into());
        }

        let mut seen_branch_ids = BTreeSet::new();
        for b in &self.branches {
            let mut unique_nodes = BTreeSet::new();
            for node in &b.nodes {
                if !seen_buses.contains(node.as_str()) {
                    problems.push(format!(
                        "ramo '{}' referencia barramento inexistente '{node}'",
                        b.label()
                    ));
                }
                if !unique_nodes.insert(node.as_str()) {
                    problems.push(format!(
                        "ramo '{}' referencia o nó '{}' mais de uma vez",
                        b.label(),
                        node
                    ));
                }
            }
            if unique_nodes.len() < 2 {
                problems.push(format!(
                    "ramo '{}' precisa interligar ao menos dois nós distintos",
                    b.label()
                ));
            }
            if let Some(id) = &b.id {
                if !seen_branch_ids.insert(id.as_str()) {
                    problems.push(format!("id de ramo duplicado: '{id}'"));
                }
                if seen_buses.contains(id.as_str()) {
                    problems.push(format!(
                        "id de ramo '{id}' colide com id de barramento/chave"
                    ));
                }
            }
        }

        // Conectividade física: ignora estado das chaves, pois a rede existente
        // deve ser uma peça só mesmo com NAs abertas em operação normal.
        if let Some(start) = self.buses.first() {
            let reachable = self.reachable_from_with(&start.id, |_| true, |_, _| true);
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

    /// Barramentos alcançáveis a partir de `start`, usando o estado normal das
    /// chaves e percorrendo apenas os ramos aceitos por `conduz_branch`.
    pub fn reachable_from<'a>(
        &'a self,
        start: &'a str,
        conduz_branch: impl Fn(&Branch) -> bool,
    ) -> BTreeSet<&'a str> {
        self.reachable_from_with(
            start,
            |b| conduz_branch(b),
            |_, bus| bus.conducts_normally(),
        )
    }

    /// Barramentos alcançáveis com controle explícito de condução de ramos e nós.
    pub fn reachable_from_with<'a, FB, FN>(
        &'a self,
        start: &'a str,
        conduz_branch: FB,
        conduz_node: FN,
    ) -> BTreeSet<&'a str>
    where
        FB: Fn(&Branch) -> bool,
        FN: Fn(usize, &Bus) -> bool,
    {
        let Some(start_idx) = self.bus_index(start) else {
            return BTreeSet::new();
        };
        let adj = self.adjacency();
        let mut visited = BTreeSet::new();
        let mut stack = vec![start_idx];

        while let Some(bus_idx) = stack.pop() {
            let bus = &self.buses[bus_idx];
            if !conduz_node(bus_idx, bus) || !visited.insert(bus.id.as_str()) {
                continue;
            }
            for &branch_idx in adj.get(bus.id.as_str()).map(Vec::as_slice).unwrap_or(&[]) {
                let branch = &self.branches[branch_idx];
                if !conduz_branch(branch) {
                    continue;
                }
                for node in &branch.nodes {
                    if node == &bus.id {
                        continue;
                    }
                    if let Some(next_idx) = self.bus_index(node) {
                        stack.push(next_idx);
                    }
                }
            }
        }

        visited
    }

    /// Índice do primeiro ramo com o id dado.
    pub fn branch_index(&self, id: &str) -> Option<usize> {
        self.branches
            .iter()
            .position(|b| b.id.as_deref() == Some(id))
    }

    /// Índice do barramento com o id dado.
    pub fn bus_index(&self, id: &str) -> Option<usize> {
        self.buses.iter().position(|b| b.id == id)
    }

    /// Índice da chave com o id dado.
    pub fn switch_index(&self, id: &str) -> Option<usize> {
        self.bus_index(id)
            .filter(|&i| matches!(self.buses[i].kind, BusKind::Switch { .. }))
    }

    /// Índices de todos os ramos-linha.
    pub fn line_indices(&self) -> BTreeSet<usize> {
        self.branches
            .iter()
            .enumerate()
            .filter(|(_, b)| matches!(b.element, Element::Line { .. }))
            .map(|(i, _)| i)
            .collect()
    }

    /// Soma de consumidores dos ramos-linha em `lines`.
    pub fn consumers_of(&self, lines: &BTreeSet<usize>) -> u32 {
        lines.iter().map(|&i| self.branches[i].consumers()).sum()
    }

    /// Ramo-linha com consumidores associado a um ponto consumidor (`bus_id`).
    ///
    /// O ponto deve existir e estar em exatamente um ramo com `consumers > 0`.
    /// Se houver mais de um, o ponto é ambíguo e o chamador deve selecionar o
    /// ramo diretamente.
    pub fn point_load_line(&self, bus_id: &str) -> Result<usize, PointLoadError> {
        if self.bus_index(bus_id).is_none() {
            return Err(PointLoadError::UnknownBus(bus_id.to_string()));
        }

        let mut matches = self
            .branches
            .iter()
            .enumerate()
            .filter(|(_, branch)| {
                branch.consumers() > 0 && branch.nodes.iter().any(|node| node == bus_id)
            })
            .map(|(i, _)| i);

        let Some(first) = matches.next() else {
            return Err(PointLoadError::NoConsumerLine(bus_id.to_string()));
        };
        if matches.next().is_some() {
            return Err(PointLoadError::AmbiguousConsumerLine(bus_id.to_string()));
        }
        Ok(first)
    }

    /// Barramentos energizados em uma configuração arbitrária.
    pub fn energized<FB, FN>(&self, conduz_branch: FB, conduz_node: FN) -> BTreeSet<&str>
    where
        FB: Fn(usize, &Branch) -> bool,
        FN: Fn(usize, &Bus) -> bool,
    {
        let mut energ = BTreeSet::new();
        for (i, source) in self.buses.iter().enumerate().filter(|(_, b)| b.is_source()) {
            if !conduz_node(i, source) {
                continue;
            }
            energ.extend(self.reachable_from_with(
                &source.id,
                |b| {
                    let idx = self
                        .branches
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, b))
                        .expect("ramo veio da própria rede");
                    conduz_branch(idx, b)
                },
                |idx, bus| conduz_node(idx, bus),
            ));
        }
        energ
    }

    /// Ramos-linha **a jusante** da chave `switch_id` na configuração normal:
    /// as linhas que perdem energia se apenas aquela chave for aberta.
    pub fn downstream_lines(&self, switch_id: &str) -> Option<BTreeSet<usize>> {
        let switch_idx = self.switch_index(switch_id)?;
        let normal = self.energized(|_, _| true, |_, bus| bus.conducts_normally());
        let cut = self.energized(
            |_, _| true,
            |i, bus| i != switch_idx && bus.conducts_normally(),
        );

        let lines = self
            .line_indices()
            .into_iter()
            .filter(|&i| self.line_served(i, &normal) && !self.line_served(i, &cut))
            .collect();
        Some(lines)
    }

    /// Um ramo-linha está servido se ao menos um de seus nós terminais está
    /// energizado. Chaves abertas não entram no conjunto `energized`.
    pub fn line_served(&self, branch_idx: usize, energized: &BTreeSet<&str>) -> bool {
        self.branches[branch_idx]
            .nodes
            .iter()
            .any(|node| energized.contains(node.as_str()))
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

/// Problemas ao resolver um barramento como ponto consumidor individual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointLoadError {
    UnknownBus(String),
    NoConsumerLine(String),
    AmbiguousConsumerLine(String),
}

impl fmt::Display for PointLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PointLoadError::UnknownBus(id) => write!(f, "barramento '{id}' não encontrado"),
            PointLoadError::NoConsumerLine(id) => write!(
                f,
                "barramento '{id}' não pertence a nenhum ramo com consumidores"
            ),
            PointLoadError::AmbiguousConsumerLine(id) => write!(
                f,
                "barramento '{id}' pertence a mais de um ramo com consumidores"
            ),
        }
    }
}

impl std::error::Error for PointLoadError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        Network(
            buses: [
                (id: "SD_A", kind: Substation),
                (id: "dA",   kind: Switch(normal: Closed)),
                (id: "a1",   kind: Junction),
                (id: "sa",   kind: Switch(normal: Closed)),
                (id: "a2",   kind: Junction),
                (id: "NA",   kind: Switch(normal: Open)),
                (id: "SD_B", kind: Substation),
                (id: "dB",   kind: Switch(normal: Closed)),
                (id: "b1",   kind: Junction),
            ],
            branches: [
                (id: Some("feed_a"), nodes: ["SD_A", "dA"], element: Line(consumers: 0)),
                (id: Some("A"),      nodes: ["dA", "a1", "sa"], element: Line(consumers: 500)),
                (id: Some("B"),      nodes: ["sa", "a2", "NA"], element: Line(consumers: 300)),
                (id: Some("feed_b"), nodes: ["SD_B", "dB"], element: Line(consumers: 0)),
                (id: Some("C"),      nodes: ["dB", "b1", "NA"], element: Line(consumers: 400)),
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
        assert_eq!(net.buses.len(), 9);
        assert_eq!(net.sources(), ["SD_A", "SD_B"]);
        net.validate().expect("rede válida");
    }

    #[test]
    fn tie_is_recognized() {
        let net = sample();
        let na = net.buses.iter().find(|b| b.id == "NA").unwrap();
        assert!(na.is_tie());
    }

    #[test]
    fn reachability_respects_open_switches() {
        let net = sample();
        let energizado = net.reachable_from("SD_A", |_| true);
        assert!(energizado.contains("a2"));
        assert!(!energizado.contains("b1"));
    }

    #[test]
    fn downstream_uses_service_loss_instead_of_all_terminals() {
        let net = sample();
        let down = net.downstream_lines("sa").unwrap();
        let ids: BTreeSet<&str> = down
            .iter()
            .filter_map(|&i| net.branches[i].id.as_deref())
            .collect();
        assert_eq!(ids, BTreeSet::from(["B"]));
    }

    #[test]
    fn rejects_reference_to_undeclared_bus() {
        let mut net = sample();
        net.branches.push(Branch {
            id: Some("solta".into()),
            nodes: vec!["a2".into(), "fantasma".into()],
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
