//! Estado e laço de UI da aplicação.

use std::collections::{HashMap, HashSet};

use decfec::fault::{Action, Event, Scenario};
use decfec::topology::{Branch, Bus, BusKind, Element, Network, State};
use egui::Pos2;
use serde::{Deserialize, Serialize};

use crate::canvas::{self, CanvasState, Selection};
use crate::engine::{self, Report};

/// Rede de referência usada como conteúdo inicial dos editores (embutida em
/// tempo de compilação — funciona em WASM, sem filesystem).
const REDE_PADRAO: &str = include_str!("../../networks/ref-exercise.ron");
const CENARIO_PADRAO: &str = include_str!("../../scenarios/item_a.ron");
const LAYOUT_PADRAO: &str = include_str!("../default-layout.ron");
const NETWORK_KEY: &str = "decfec.network.ron.v1";
const SCENARIO_KEY: &str = "decfec.scenario.ron.v1";
const SELECTED_SET_KEY: &str = "decfec.selected_set.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetworkDocument {
    network: Network,
    #[serde(default)]
    layout: SavedCanvasPositions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SavedCanvasPositions {
    positions: Vec<SavedNodePosition>,
    #[serde(default)]
    hidden_bus_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedNodePosition {
    id: String,
    x: f32,
    y: f32,
}

/// Aplicação egui.
///
/// Nesta etapa o fluxo é dirigido por texto RON (rede + cenário). O editor de
/// grafo em canvas entra nas próximas etapas, mas já reaproveitará `net`.
pub struct App {
    /// Texto RON da rede + layout do canvas (editável).
    net_ron: String,
    /// Texto RON do cenário de faltas (para importar/recarregar).
    scenario_ron: String,
    /// Cenário em memória (fonte da verdade para o editor de eventos).
    scenario: Scenario,
    /// Chave para o conjunto a jusante; vazio = sistema inteiro.
    switch: String,

    /// Rede carregada e validada (ou `None` se o último parse falhou).
    net: Option<Network>,
    /// Posições dos nós (coordenadas-mundo) — só na UI, derivadas no carregamento.
    positions: HashMap<String, Pos2>,
    /// Estado de câmera/seleção do canvas.
    canvas: CanvasState,
    /// Mensagem do último carregamento da rede (erro, ou resumo de sucesso).
    net_status: Result<String, String>,
    /// Resultado da última simulação.
    report: Option<Result<Report, String>>,
    /// Texto em edição para os terminais do ramo selecionado.
    branch_nodes_editor: Option<(usize, String)>,
    /// Texto em edição para o id da barra selecionada.
    bus_id_editor: Option<(String, String)>,
    /// Labels de barramentos ocultos no canvas (estado visual da GUI).
    hidden_bus_labels: HashSet<String>,
    /// Se `true`, grava `hidden_bus_labels` no storage no fim do frame atual.
    hidden_bus_labels_dirty: bool,
    /// Se `true`, grava a rede editada no storage no fim do frame atual.
    network_dirty: bool,
    /// Se `true`, grava o cenário/eventos no storage no fim do frame atual.
    scenario_dirty: bool,
    /// Se `true`, grava o conjunto selecionado no storage no fim do frame atual.
    selected_set_dirty: bool,
}

impl App {
    /// Constrói a aplicação a partir do contexto de criação do eframe.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            net_ron: default_network_document_ron(),
            scenario_ron: CENARIO_PADRAO.to_string(),
            scenario: engine::load_scenario(CENARIO_PADRAO)
                .unwrap_or_else(|_| Scenario { events: Vec::new() }),
            switch: "2".to_string(),
            net: None,
            positions: HashMap::new(),
            canvas: CanvasState::default(),
            net_status: Ok(String::new()),
            report: None,
            branch_nodes_editor: None,
            bus_id_editor: None,
            hidden_bus_labels: HashSet::new(),
            hidden_bus_labels_dirty: false,
            network_dirty: false,
            scenario_dirty: false,
            selected_set_dirty: false,
        };
        let restored_network = cc
            .storage
            .is_some_and(|storage| app.restore_network(storage));
        if let Some(storage) = cc.storage {
            app.restore_scenario(storage);
            app.restore_selected_set(storage);
        }
        if !restored_network {
            app.load_network();
        }
        app
    }

    /// (Re)carrega a rede a partir do texto RON, atualizando `net`/`net_status`.
    fn load_network(&mut self) {
        match load_network_document(&self.net_ron) {
            Ok(doc) => {
                self.net_status = Ok(network_summary(&doc.network));
                self.positions = canvas::layout(&doc.network);
                self.net = Some(doc.network);
                self.apply_saved_canvas_layout(doc.layout);
            }
            Err(e) => {
                self.net = None;
                self.positions.clear();
                self.net_status = Err(e);
            }
        }
        // Rede mudou: reenquadra, limpa seleção e invalida o resultado anterior.
        self.canvas.request_fit();
        self.canvas.clear_selection();
        self.report = None;
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.retain_hidden_labels_for_loaded_network();
    }

    /// Roda a simulação com a rede carregada e o cenário/chave atuais.
    fn simulate(&mut self) {
        let switch = self.switch.trim();
        self.report = Some(match &self.net {
            Some(net) => engine::run(net, &self.scenario, Some(switch)),
            None => Err("carregue uma rede válida antes de simular".to_string()),
        });
    }

    /// Reparseia o cenário a partir do texto RON (botão "Recarregar").
    fn load_scenario_from_ron(&mut self) {
        match engine::load_scenario(&self.scenario_ron) {
            Ok(s) => {
                self.scenario = s;
                self.report = None;
                self.mark_scenario_dirty();
            }
            Err(e) => self.report = Some(Err(e)),
        }
    }

    fn restore_network(&mut self, storage: &dyn eframe::Storage) -> bool {
        let Some(text) = storage.get_string(NETWORK_KEY) else {
            return false;
        };
        let Ok(doc) = load_network_document(&text) else {
            return false;
        };

        self.net_ron = text;
        self.positions = canvas::layout(&doc.network);
        self.net = Some(doc.network);
        self.apply_saved_canvas_layout(doc.layout);
        self.revalidate();
        true
    }

    fn save_network(&self, storage: &mut dyn eframe::Storage) {
        if let Ok(text) = self.network_document_to_ron() {
            storage.set_string(NETWORK_KEY, text);
        }
    }

    fn restore_scenario(&mut self, storage: &dyn eframe::Storage) {
        let Some(text) = storage.get_string(SCENARIO_KEY) else {
            return;
        };
        if let Ok(scenario) = engine::load_scenario(&text) {
            self.scenario = scenario;
            self.scenario_ron = text;
        }
    }

    fn save_scenario(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(SCENARIO_KEY, engine::scenario_to_ron(&self.scenario));
    }

    fn restore_selected_set(&mut self, storage: &dyn eframe::Storage) {
        if let Some(selected_set) = storage.get_string(SELECTED_SET_KEY) {
            self.switch = selected_set;
        }
    }

    fn save_selected_set(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(SELECTED_SET_KEY, self.switch.clone());
    }

    fn flush_state(&mut self, frame: &mut eframe::Frame) {
        if !self.network_dirty
            && !self.scenario_dirty
            && !self.selected_set_dirty
            && !self.hidden_bus_labels_dirty
        {
            return;
        }
        if let Some(storage) = frame.storage_mut() {
            if self.network_dirty || self.hidden_bus_labels_dirty {
                self.save_network(storage);
                self.network_dirty = false;
                self.hidden_bus_labels_dirty = false;
            }
            if self.scenario_dirty {
                self.save_scenario(storage);
                self.scenario_dirty = false;
            }
            if self.selected_set_dirty {
                self.save_selected_set(storage);
                self.selected_set_dirty = false;
            }
        }
    }

    fn apply_saved_canvas_layout(&mut self, saved: SavedCanvasPositions) -> usize {
        let mut applied = 0;
        for p in saved.positions {
            if let std::collections::hash_map::Entry::Occupied(mut entry) =
                self.positions.entry(p.id)
            {
                entry.insert(Pos2::new(p.x, p.y));
                applied += 1;
            }
        }
        self.apply_hidden_bus_labels(saved.hidden_bus_labels);
        applied
    }

    fn retain_hidden_labels_for_loaded_network(&mut self) {
        let Some(net) = &self.net else {
            return;
        };
        let before = self.hidden_bus_labels.len();
        self.hidden_bus_labels
            .retain(|id| net.buses.iter().any(|bus| bus.id == *id));
        if self.hidden_bus_labels.len() != before {
            self.hidden_bus_labels_dirty = true;
        }
    }

    fn mark_hidden_bus_labels_dirty(&mut self) {
        self.hidden_bus_labels_dirty = true;
    }

    fn mark_network_dirty(&mut self) {
        self.network_dirty = true;
    }

    fn mark_scenario_dirty(&mut self) {
        self.scenario_dirty = true;
    }

    fn mark_selected_set_dirty(&mut self) {
        self.selected_set_dirty = true;
    }

    fn apply_hidden_bus_labels(&mut self, ids: Vec<String>) {
        let Some(net) = &self.net else {
            self.hidden_bus_labels.clear();
            self.hidden_bus_labels_dirty = true;
            return;
        };
        let next: HashSet<String> = ids
            .into_iter()
            .filter(|id| net.buses.iter().any(|bus| bus.id == *id))
            .collect();
        if self.hidden_bus_labels != next {
            self.hidden_bus_labels = next;
            self.hidden_bus_labels_dirty = true;
        }
    }

    fn saved_canvas_positions(&self) -> SavedCanvasPositions {
        let mut positions: Vec<SavedNodePosition> = self
            .positions
            .iter()
            .map(|(id, p)| SavedNodePosition {
                id: id.clone(),
                x: p.x,
                y: p.y,
            })
            .collect();
        positions.sort_by(|a, b| a.id.cmp(&b.id));

        let mut hidden_bus_labels: Vec<String> = self.hidden_bus_labels.iter().cloned().collect();
        hidden_bus_labels.sort();

        SavedCanvasPositions {
            positions,
            hidden_bus_labels,
        }
    }

    fn network_document_to_ron(&self) -> Result<String, ron::Error> {
        let Some(net) = &self.net else {
            return Ok(self.net_ron.clone());
        };
        network_document_to_ron(&NetworkDocument {
            network: net.clone(),
            layout: self.saved_canvas_positions(),
        })
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("titulo").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.heading("DEC/FEC: Calculadora de Indicadores");
            ui.add_space(4.0);
        });

        egui::Panel::left("painel_rede")
            .resizable(true)
            .default_size(400.0)
            .show_inside(ui, |ui| self.painel_rede(ui));

        egui::Panel::right("painel_cenario")
            .resizable(true)
            .default_size(380.0)
            .show_inside(ui, |ui| self.painel_cenario(ui));

        egui::CentralPanel::default().show_inside(ui, |ui| self.painel_canvas(ui));

        self.flush_state(frame);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_network(storage);
        self.save_scenario(storage);
        self.save_selected_set(storage);
    }
}

impl App {
    /// Painel esquerdo: editor da seleção + status + editor RON (colapsável).
    fn painel_rede(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong("Rede");
            if ui.button("Recarregar do RON").clicked() {
                self.load_network();
                self.mark_network_dirty();
            }
            if ui.button("Novo vazio").clicked() {
                self.reset_empty_network();
            }
        });
        match &self.net_status {
            Ok(resumo) if !resumo.is_empty() => {
                ui.colored_label(egui::Color32::from_rgb(120, 200, 120), resumo);
            }
            Ok(_) => {}
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(230, 120, 120), e);
            }
        }

        ui.separator();
        ui.strong("Seleção");
        self.painel_edicao(ui);

        ui.separator();
        egui::CollapsingHeader::new("Texto RON")
            .default_open(false)
            .show(ui, |ui| {
                if ui.button("Exportar do grafo").clicked()
                    && let Ok(text) = self.network_document_to_ron()
                {
                    self.net_ron = text;
                }
                egui::ScrollArea::both()
                    .id_salt("scroll_rede")
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.net_ron)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(20),
                        );
                    });
            });
    }

    fn reset_empty_network(&mut self) {
        let net = Network {
            buses: Vec::new(),
            branches: Vec::new(),
        };
        self.net = Some(net);
        self.positions.clear();
        self.canvas.request_fit();
        self.canvas.clear_selection();
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        if !self.hidden_bus_labels.is_empty() {
            self.hidden_bus_labels.clear();
            self.mark_hidden_bus_labels_dirty();
        }
        self.report = None;
        self.net_status = Ok("canvas vazio".to_string());
        if let Ok(text) = self.network_document_to_ron() {
            self.net_ron = text;
        }
        self.mark_network_dirty();
    }

    /// Editor da entidade selecionada no canvas (barra ou ramo).
    ///
    /// Edita a rede em memória diretamente (que passa a ser a fonte da verdade
    /// após manobras gráficas); o texto RON só é reimportado via "Recarregar".
    fn painel_edicao(&mut self, ui: &mut egui::Ui) {
        let selections = self.canvas.selections.clone();
        let Some(sel) = selections.first().cloned() else {
            ui.weak("Clique num nó ou ramo no grafo para editar.");
            return;
        };
        if selections.len() > 1 {
            ui.label(format!("{} itens selecionados", selections.len()));
            ui.weak("Use + Ramo para conectar as barras selecionadas, ou Excluir seleção.");
            return;
        }
        let Some(net) = self.net.as_mut() else {
            return;
        };

        match sel {
            Selection::Bus(id) => {
                self.branch_nodes_editor = None;
                if self
                    .bus_id_editor
                    .as_ref()
                    .is_none_or(|(selected, _)| selected != &id)
                {
                    self.bus_id_editor = Some((id.clone(), id.clone()));
                }

                let Some(bus_idx) = net.buses.iter().position(|b| b.id == id) else {
                    return;
                };
                ui.label("Barramento");

                let id_buf = &mut self
                    .bus_id_editor
                    .as_mut()
                    .expect("editor inicializado acima")
                    .1;
                let mut rename_to = None;
                if ui
                    .horizontal(|ui| {
                        ui.label("id:");
                        ui.text_edit_singleline(id_buf)
                    })
                    .inner
                    .changed()
                {
                    let new_id = id_buf.trim();
                    if !new_id.is_empty() && new_id != id {
                        rename_to = Some(new_id.to_string());
                    }
                }

                if let Some(new_id) = rename_to {
                    let old_id = id.clone();
                    net.buses[bus_idx].id = new_id.clone();
                    for branch in &mut net.branches {
                        for node in &mut branch.nodes {
                            if node == &old_id {
                                *node = new_id.clone();
                            }
                        }
                    }
                    if let Some(pos) = self.positions.remove(&old_id) {
                        self.positions.insert(new_id.clone(), pos);
                    }
                    if self.hidden_bus_labels.remove(&old_id) {
                        self.hidden_bus_labels.insert(new_id.clone());
                        self.hidden_bus_labels_dirty = true;
                    }
                    if self.switch == old_id {
                        self.switch = new_id.clone();
                    }
                    for event in &mut self.scenario.events {
                        if event.bus == old_id {
                            event.bus = new_id.clone();
                        }
                    }
                    self.canvas.set_selection(Selection::Bus(new_id.clone()));
                    self.bus_id_editor = Some((new_id.clone(), new_id));
                    self.report = None;
                    self.mark_network_dirty();
                    self.mark_scenario_dirty();
                    return self.revalidate();
                }

                let mut show_label = !self.hidden_bus_labels.contains(&id);
                if ui
                    .checkbox(&mut show_label, "Exibir label no grafo")
                    .changed()
                {
                    if show_label {
                        if self.hidden_bus_labels.remove(&id) {
                            self.hidden_bus_labels_dirty = true;
                        }
                    } else {
                        if self.hidden_bus_labels.insert(id.clone()) {
                            self.hidden_bus_labels_dirty = true;
                        }
                    }
                }

                let bus = &mut net.buses[bus_idx];
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("Tipo:");
                    if ui
                        .selectable_label(bus.kind == BusKind::Substation, "Subestação")
                        .clicked()
                    {
                        bus.kind = BusKind::Substation;
                        changed = true;
                    }
                    if ui
                        .selectable_label(bus.kind == BusKind::Junction, "Junção")
                        .clicked()
                    {
                        bus.kind = BusKind::Junction;
                        changed = true;
                    }
                    if ui
                        .selectable_label(matches!(bus.kind, BusKind::Switch { .. }), "Chave")
                        .clicked()
                        && !matches!(bus.kind, BusKind::Switch { .. })
                    {
                        bus.kind = BusKind::Switch {
                            normal: State::Closed,
                        };
                        changed = true;
                    }
                });
                if let BusKind::Switch { normal } = &mut bus.kind {
                    ui.horizontal(|ui| {
                        ui.label("Normal:");
                        changed |= ui
                            .radio_value(normal, State::Closed, "NF (fechada)")
                            .changed();
                        changed |= ui
                            .radio_value(normal, State::Open, "NA / tie (aberta)")
                            .changed();
                    });
                }
                if changed {
                    self.network_dirty = true;
                }
            }
            Selection::Branch(i) => {
                self.bus_id_editor = None;
                let Some(b) = net.branches.get_mut(i) else {
                    return;
                };
                if self
                    .branch_nodes_editor
                    .as_ref()
                    .is_none_or(|(idx, _)| *idx != i)
                {
                    self.branch_nodes_editor = Some((i, b.nodes.join(", ")));
                }
                ui.label("Ramo");

                let nodes_buf = &mut self
                    .branch_nodes_editor
                    .as_mut()
                    .expect("editor inicializado acima")
                    .1;
                if ui
                    .horizontal(|ui| {
                        ui.label("nós:");
                        ui.text_edit_singleline(nodes_buf)
                    })
                    .inner
                    .changed()
                {
                    b.nodes = parse_branch_nodes(nodes_buf);
                    self.network_dirty = true;
                }

                match &mut b.element {
                    Element::Line { consumers } => {
                        if ui
                            .add(egui::DragValue::new(consumers).prefix("consumidores: "))
                            .changed()
                        {
                            self.network_dirty = true;
                        }
                    }
                }
            }
        }

        // Revalida após a edição para sinalizar problemas (duplicatas, laços…).
        self.revalidate();
    }

    /// Revalida a rede em memória e atualiza a mensagem de status.
    fn revalidate(&mut self) {
        if let Some(net) = &self.net {
            self.net_status = match net.validate() {
                Ok(()) => Ok(network_summary(net)),
                Err(e) => Err(e.to_string()),
            };
        }
    }

    /// Painel central: toolbar de estrutura + o grafo (arrastar/pan/zoom/selecionar).
    fn painel_canvas(&mut self, ui: &mut egui::Ui) {
        // Toolbar primeiro (muta `self`), antes do empréstimo de `self.net`.
        ui.horizontal_wrapped(|ui| {
            if ui.button("Ajustar à vista").clicked() {
                self.canvas.request_fit();
            }
            if ui.button("Auto-layout").clicked() {
                self.relayout();
            }
            ui.separator();
            if ui.button("+ Barra").clicked() {
                self.add_bus();
            }
            let pode_ramo = self.net.as_ref().is_some_and(|n| n.buses.len() >= 2);
            if ui
                .add_enabled(pode_ramo, egui::Button::new("+ Ramo"))
                .clicked()
            {
                self.add_branch();
            }
            let tem_sel = !self.canvas.selections.is_empty();
            if ui
                .add_enabled(tem_sel, egui::Button::new("Excluir seleção"))
                .clicked()
            {
                self.delete_selection();
            }
        });
        ui.weak("arraste nós · arraste o fundo p/ mover · roda p/ zoom · clique p/ selecionar");

        let Some(net) = &self.net else {
            ui.centered_and_justified(|ui| {
                ui.weak("Carregue uma rede válida (painel à esquerda) para vê-la aqui.");
            });
            return;
        };
        canvas::draw(
            ui,
            net,
            &mut self.positions,
            &mut self.canvas,
            &self.hidden_bus_labels,
        );

        let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete));
        if delete_pressed
            && !ui.ctx().egui_wants_keyboard_input()
            && !self.canvas.selections.is_empty()
        {
            self.delete_selection();
        }
    }

    /// Recalcula o layout automático e reenquadra (útil após importar/editar).
    fn relayout(&mut self) {
        if let Some(net) = &self.net {
            self.positions = canvas::layout(net);
            self.canvas.request_fit();
        }
    }

    /// Adiciona um barramento novo (junção) perto do centro da vista e o seleciona.
    fn add_bus(&mut self) {
        let pos = self.canvas.insertion_pos();
        let Some(net) = self.net.as_mut() else {
            return;
        };
        let id = unique_id(net.buses.iter().map(|b| b.id.as_str()), "b");
        net.buses.push(Bus {
            id: id.clone(),
            kind: BusKind::Junction,
        });
        self.positions.insert(id.clone(), pos);
        self.canvas.set_selection(Selection::Bus(id));
        self.revalidate();
        self.mark_network_dirty();
    }

    /// Adiciona um ramo entre as barras selecionadas, ou entre as duas últimas.
    fn add_branch(&mut self) {
        let selected_nodes: Vec<String> = self
            .canvas
            .selections
            .iter()
            .filter_map(|sel| match sel {
                Selection::Bus(id) => Some(id.clone()),
                Selection::Branch(_) => None,
            })
            .collect();
        let Some(net) = self.net.as_mut() else {
            return;
        };
        let nodes = if selected_nodes.len() >= 2 {
            selected_nodes
        } else if net.buses.len() >= 2 {
            net.buses[net.buses.len() - 2..]
                .iter()
                .map(|b| b.id.clone())
                .collect()
        } else {
            return;
        };
        let id = unique_id(net.branches.iter().filter_map(|b| b.id.as_deref()), "ramo");
        net.branches.push(Branch {
            id: Some(id),
            nodes,
            element: Element::Line { consumers: 0 },
        });
        self.canvas
            .set_selection(Selection::Branch(net.branches.len() - 1));
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.revalidate();
        self.mark_network_dirty();
    }

    /// Exclui a seleção: ramos e/ou barras (em cascata com seus ramos).
    fn delete_selection(&mut self) {
        let selections = std::mem::take(&mut self.canvas.selections);
        if selections.is_empty() {
            return;
        };
        let Some(net) = self.net.as_mut() else {
            return;
        };
        let buses_to_remove: HashSet<String> = selections
            .iter()
            .filter_map(|sel| match sel {
                Selection::Bus(id) => Some(id.clone()),
                Selection::Branch(_) => None,
            })
            .collect();
        let branches_to_remove: HashSet<usize> = selections
            .iter()
            .filter_map(|sel| match sel {
                Selection::Branch(i) => Some(*i),
                Selection::Bus(_) => None,
            })
            .collect();

        net.buses.retain(|b| !buses_to_remove.contains(&b.id));
        let mut branch_idx = 0;
        net.branches.retain(|b| {
            let remove = branches_to_remove.contains(&branch_idx)
                || b.nodes.iter().any(|n| buses_to_remove.contains(n));
            branch_idx += 1;
            !remove
        });
        for id in buses_to_remove {
            self.positions.remove(&id);
            if self.hidden_bus_labels.remove(&id) {
                self.mark_hidden_bus_labels_dirty();
            }
            if self.switch == id {
                self.switch.clear();
                self.mark_selected_set_dirty();
            }
        }
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.revalidate();
        self.mark_network_dirty();
    }

    /// Painel direito: seleção de conjunto, resultados e editor de eventos.
    fn painel_cenario(&mut self, ui: &mut egui::Ui) {
        // Ids auxiliares, clonados para não conflitar com `&mut self.scenario`.
        let switch_ids: Vec<String> = self
            .net
            .iter()
            .flat_map(|n| n.buses.iter().filter(|b| b.is_switch()))
            .map(|b| b.id.clone())
            .collect();
        let bus_ids: Vec<String> = self
            .net
            .iter()
            .flat_map(|n| n.buses.iter())
            .map(|b| b.id.clone())
            .collect();

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong("Conjunto:");
            let texto = if self.switch.is_empty() {
                "Sistema inteiro".to_string()
            } else {
                format!("a jusante de {}", self.switch)
            };
            let conjunto_resp = egui::ComboBox::from_id_salt("conjunto")
                .selected_text(texto)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.switch, String::new(), "Sistema inteiro");
                    for id in &switch_ids {
                        ui.selectable_value(&mut self.switch, id.clone(), id);
                    }
                });
            if conjunto_resp.response.changed() {
                self.mark_selected_set_dirty();
            }
            if ui.button("▶ Simular").clicked() {
                self.simulate();
            }
        });

        self.painel_resultado(ui);

        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Eventos");
            if ui.button("+ adicionar").clicked() {
                self.scenario.events.push(Event {
                    at_min: 0.0,
                    bus: bus_ids.first().cloned().unwrap_or_default(),
                    action: Action::Fault,
                });
                self.mark_scenario_dirty();
            }
            if ui.button("ordenar por tempo").clicked() {
                self.scenario
                    .events
                    .sort_by(|a, b| a.at_min.total_cmp(&b.at_min));
                self.mark_scenario_dirty();
            }
        });

        let mut remover: Option<usize> = None;
        let mut scenario_changed = false;
        let eventos_max_height = (ui.available_height() - 48.0).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("scroll_eventos")
            .max_height(eventos_max_height)
            .show(ui, |ui| {
                for (i, ev) in self.scenario.events.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        scenario_changed |= ui
                            .add(
                                egui::DragValue::new(&mut ev.at_min)
                                    .suffix(" min")
                                    .range(0.0..=f64::MAX)
                                    .speed(1.0),
                            )
                            .changed();
                        egui::ComboBox::from_id_salt(("ev_bus", i))
                            .selected_text(ev.bus.clone())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for id in &bus_ids {
                                    scenario_changed |=
                                        ui.selectable_value(&mut ev.bus, id.clone(), id).changed();
                                }
                            });
                        egui::ComboBox::from_id_salt(("ev_action", i))
                            .selected_text(action_label(ev.action))
                            .show_ui(ui, |ui| {
                                for a in
                                    [Action::Fault, Action::Repair, Action::Open, Action::Close]
                                {
                                    scenario_changed |= ui
                                        .selectable_value(&mut ev.action, a, action_label(a))
                                        .changed();
                                }
                            });
                        if ui.small_button("X").clicked() {
                            remover = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = remover {
            self.scenario.events.remove(i);
            scenario_changed = true;
        }
        if scenario_changed {
            self.mark_scenario_dirty();
        }

        ui.separator();
        egui::CollapsingHeader::new("Texto RON")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Recarregar do RON").clicked() {
                        self.load_scenario_from_ron();
                    }
                    if ui.button("Exportar").clicked() {
                        self.scenario_ron = engine::scenario_to_ron(&self.scenario);
                    }
                });
                egui::ScrollArea::both()
                    .id_salt("scroll_cenario")
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.scenario_ron)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(12),
                        );
                    });
            });
    }

    /// Caixa de resultados DEC/FEC (ou erro) da última simulação.
    fn painel_resultado(&self, ui: &mut egui::Ui) {
        let Some(report) = &self.report else {
            return;
        };
        ui.separator();
        match report {
            Ok(r) => {
                ui.label(format!("Conjunto: {} - Cc = {} consumidores", r.alvo, r.cc));
                ui.horizontal(|ui| {
                    ui.heading(format!("DEC = {:.3} h", r.ind.dec_h));
                    ui.add_space(16.0);
                    ui.heading(format!("FEC = {:.3}", r.ind.fec));
                });
                ui.weak(format!("({:.1} min)", r.ind.dec_h * 60.0));
            }
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(230, 120, 120), e);
            }
        }
    }
}

/// Primeiro id `{prefixo}{n}` (n≥1) que não colide com os existentes.
fn unique_id<'a>(existing: impl Iterator<Item = &'a str>, prefixo: &str) -> String {
    let usados: std::collections::HashSet<&str> = existing.collect();
    (1..)
        .map(|n| format!("{prefixo}{n}"))
        .find(|id| !usados.contains(id.as_str()))
        .expect("sequência infinita sempre acha um id livre")
}

fn default_network_document_ron() -> String {
    let network = Network::from_ron(REDE_PADRAO).expect("rede padrão deve carregar");
    let layout =
        ron::from_str::<SavedCanvasPositions>(LAYOUT_PADRAO).expect("layout padrão deve carregar");
    network_document_to_ron(&NetworkDocument { network, layout })
        .expect("documento padrão deve serializar")
}

fn load_network_document(text: &str) -> Result<NetworkDocument, String> {
    let doc = ron::from_str::<NetworkDocument>(text)
        .map_err(|e| format!("erro de parse na rede/layout: {e}"))?;
    doc.network.validate().map_err(|e| e.to_string())?;
    Ok(doc)
}

fn network_document_to_ron(doc: &NetworkDocument) -> Result<String, ron::Error> {
    let cfg = ron::ser::PrettyConfig::default();
    ron::ser::to_string_pretty(doc, cfg)
}

fn network_summary(net: &Network) -> String {
    format!(
        "{} Subestações, {} Chaves, Cc total = {}",
        net.sources().len(),
        net.switches().len(),
        net.total_consumers()
    )
}

fn parse_branch_nodes(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Rótulo em português de uma ação de evento.
fn action_label(a: Action) -> &'static str {
    match a {
        Action::Fault => "Falta",
        Action::Repair => "Reparo",
        Action::Open => "Abrir",
        Action::Close => "Fechar",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documento_de_rede_aceita_layout_omitido() {
        let doc = load_network_document(&format!("(network: {REDE_PADRAO})"))
            .expect("layout deve ser opcional");

        assert!(!doc.network.buses.is_empty());
        assert!(doc.layout.positions.is_empty());
    }
}
