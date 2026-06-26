//! Estado e laço de UI da aplicação.

use std::collections::{HashMap, HashSet};

use decfec::fault::{Action, Event, Scenario};
use decfec::topology::{Branch, Bus, BusKind, Element, Network, State};
use egui::{Pos2, Vec2};
use serde::{Deserialize, Serialize};

use crate::canvas::{self, CanvasState, Selection};
use crate::engine::{self, Report};

/// Rede de referência usada como conteúdo inicial dos editores (embutida em
/// tempo de compilação — funciona em WASM, sem filesystem).
const REDE_PADRAO: &str = include_str!("../../networks/ref-exercise.ron");
const CENARIO_PADRAO: &str = include_str!("../../scenarios/item_a.ron");
const CANVAS_POSITIONS_KEY: &str = "decfec.canvas.positions.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedCanvasPositions {
    positions: Vec<SavedNodePosition>,
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
    /// Texto RON da rede (editável).
    net_ron: String,
    /// Texto RON do cenário de faltas (para importar/recarregar).
    scenario_ron: String,
    /// Texto RON do layout do canvas (posições dos nós).
    layout_ron: String,
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
    /// Mensagem do último import/export de layout.
    layout_status: Option<Result<String, String>>,
    /// Texto em edição para os terminais do ramo selecionado.
    branch_nodes_editor: Option<(usize, String)>,
    /// Texto em edição para o id da barra selecionada.
    bus_id_editor: Option<(String, String)>,
    /// Labels de barramentos ocultos no canvas (estado visual da GUI).
    hidden_bus_labels: HashSet<String>,
}

impl App {
    /// Constrói a aplicação a partir do contexto de criação do eframe.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            net_ron: REDE_PADRAO.to_string(),
            scenario_ron: CENARIO_PADRAO.to_string(),
            layout_ron: String::new(),
            scenario: engine::load_scenario(CENARIO_PADRAO)
                .unwrap_or_else(|_| Scenario { events: Vec::new() }),
            switch: "2".to_string(),
            net: None,
            positions: HashMap::new(),
            canvas: CanvasState::default(),
            net_status: Ok(String::new()),
            report: None,
            layout_status: None,
            branch_nodes_editor: None,
            bus_id_editor: None,
            hidden_bus_labels: HashSet::new(),
        };
        app.load_network();
        if let Some(storage) = cc.storage {
            app.restore_canvas_positions(storage);
        }
        app
    }

    /// (Re)carrega a rede a partir do texto RON, atualizando `net`/`net_status`.
    fn load_network(&mut self) {
        match engine::load_network(&self.net_ron) {
            Ok(net) => {
                self.net_status = Ok(format!(
                    "{} barramentos, {} ramos, Cc total = {}",
                    net.buses.len(),
                    net.branches.len(),
                    net.total_consumers()
                ));
                self.positions = canvas::layout(&net);
                self.net = Some(net);
            }
            Err(e) => {
                self.net = None;
                self.positions.clear();
                self.net_status = Err(e);
            }
        }
        // Rede mudou: reenquadra, limpa seleção e invalida o resultado anterior.
        self.canvas.request_fit();
        self.canvas.selection = None;
        self.report = None;
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.hidden_bus_labels.clear();
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
            }
            Err(e) => self.report = Some(Err(e)),
        }
    }

    fn restore_canvas_positions(&mut self, storage: &dyn eframe::Storage) {
        let Some(saved) = storage.get_string(CANVAS_POSITIONS_KEY) else {
            return;
        };
        let Ok(saved) = ron::from_str::<SavedCanvasPositions>(&saved) else {
            return;
        };
        for p in saved.positions {
            if let std::collections::hash_map::Entry::Occupied(mut entry) =
                self.positions.entry(p.id)
            {
                entry.insert(Pos2::new(p.x, p.y));
            }
        }
    }

    fn save_canvas_positions(&self, storage: &mut dyn eframe::Storage) {
        if let Ok(text) = self.canvas_positions_to_ron() {
            storage.set_string(CANVAS_POSITIONS_KEY, text);
        }
    }

    fn canvas_positions_to_ron(&self) -> Result<String, ron::Error> {
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

        let cfg = ron::ser::PrettyConfig::default();
        ron::ser::to_string_pretty(&SavedCanvasPositions { positions }, cfg)
    }

    fn export_canvas_layout(&mut self) {
        match self.canvas_positions_to_ron() {
            Ok(text) => {
                self.layout_ron = text;
                self.layout_status =
                    Some(Ok(format!("{} posições exportadas", self.positions.len())));
            }
            Err(e) => self.layout_status = Some(Err(format!("erro ao exportar layout: {e}"))),
        }
    }

    fn import_canvas_layout(&mut self) {
        let saved = match ron::from_str::<SavedCanvasPositions>(&self.layout_ron) {
            Ok(saved) => saved,
            Err(e) => {
                self.layout_status = Some(Err(format!("erro de parse no layout: {e}")));
                return;
            }
        };

        let mut applied = 0;
        for p in saved.positions {
            if let std::collections::hash_map::Entry::Occupied(mut entry) =
                self.positions.entry(p.id)
            {
                entry.insert(Pos2::new(p.x, p.y));
                applied += 1;
            }
        }
        self.canvas.request_fit();
        self.layout_status = Some(Ok(format!("{applied} posições aplicadas")));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("titulo").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.heading("decfec — editor de redes e falhas");
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
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_canvas_positions(storage);
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
            }
            if ui.button("Novo vazio").clicked() {
                self.reset_empty_network();
            }
        });
        match &self.net_status {
            Ok(resumo) if !resumo.is_empty() => {
                ui.colored_label(
                    egui::Color32::from_rgb(120, 200, 120),
                    format!("✓ {resumo}"),
                );
            }
            Ok(_) => {}
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(230, 120, 120), format!("✗ {e}"));
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
                    && let Some(net) = &self.net
                {
                    self.net_ron = engine::network_to_ron(net);
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

        ui.separator();
        egui::CollapsingHeader::new("Layout do canvas")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Exportar layout").clicked() {
                        self.export_canvas_layout();
                    }
                    if ui.button("Recarregar layout").clicked() {
                        self.import_canvas_layout();
                    }
                });
                if let Some(status) = &self.layout_status {
                    match status {
                        Ok(msg) => ui.colored_label(egui::Color32::from_rgb(120, 200, 120), msg),
                        Err(e) => ui.colored_label(egui::Color32::from_rgb(230, 120, 120), e),
                    };
                }
                egui::ScrollArea::both()
                    .id_salt("scroll_layout")
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.layout_ron)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(10),
                        );
                    });
            });
    }

    fn reset_empty_network(&mut self) {
        let net = Network {
            buses: Vec::new(),
            branches: Vec::new(),
        };
        self.net_ron = engine::network_to_ron(&net);
        self.net = Some(net);
        self.positions.clear();
        self.canvas.request_fit();
        self.canvas.selection = None;
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.hidden_bus_labels.clear();
        self.report = None;
        self.net_status = Ok("canvas vazio".to_string());
    }

    /// Editor da entidade selecionada no canvas (barra ou ramo).
    ///
    /// Edita a rede em memória diretamente (que passa a ser a fonte da verdade
    /// após manobras gráficas); o texto RON só é reimportado via "Recarregar".
    fn painel_edicao(&mut self, ui: &mut egui::Ui) {
        let Some(sel) = self.canvas.selection.clone() else {
            ui.weak("Clique num nó ou ramo no grafo para editar.");
            return;
        };
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
                    }
                    if self.switch == old_id {
                        self.switch = new_id.clone();
                    }
                    for event in &mut self.scenario.events {
                        if event.bus == old_id {
                            event.bus = new_id.clone();
                        }
                    }
                    self.canvas.selection = Some(Selection::Bus(new_id.clone()));
                    self.bus_id_editor = Some((new_id.clone(), new_id));
                    self.report = None;
                    return self.revalidate();
                }

                let mut show_label = !self.hidden_bus_labels.contains(&id);
                if ui
                    .checkbox(&mut show_label, "Exibir label no grafo")
                    .changed()
                {
                    if show_label {
                        self.hidden_bus_labels.remove(&id);
                    } else {
                        self.hidden_bus_labels.insert(id.clone());
                    }
                }

                let bus = &mut net.buses[bus_idx];
                ui.horizontal(|ui| {
                    ui.label("Tipo:");
                    if ui
                        .selectable_label(bus.kind == BusKind::Substation, "Subestação")
                        .clicked()
                    {
                        bus.kind = BusKind::Substation;
                    }
                    if ui
                        .selectable_label(bus.kind == BusKind::Junction, "Junção")
                        .clicked()
                    {
                        bus.kind = BusKind::Junction;
                    }
                    if ui
                        .selectable_label(matches!(bus.kind, BusKind::Switch { .. }), "Chave")
                        .clicked()
                        && !matches!(bus.kind, BusKind::Switch { .. })
                    {
                        bus.kind = BusKind::Switch {
                            normal: State::Closed,
                        };
                    }
                });
                if let BusKind::Switch { normal } = &mut bus.kind {
                    ui.horizontal(|ui| {
                        ui.label("Normal:");
                        ui.radio_value(normal, State::Closed, "NF (fechada)");
                        ui.radio_value(normal, State::Open, "NA / tie (aberta)");
                    });
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
                }

                match &mut b.element {
                    Element::Line { consumers } => {
                        ui.add(egui::DragValue::new(consumers).prefix("consumidores: "));
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
                Ok(()) => Ok(format!(
                    "{} barramentos, {} ramos, Cc total = {}",
                    net.buses.len(),
                    net.branches.len(),
                    net.total_consumers()
                )),
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
            let tem_sel = self.canvas.selection.is_some();
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
        let pos = centroid(&self.positions) + Vec2::new(60.0, 0.0);
        let Some(net) = self.net.as_mut() else {
            return;
        };
        let id = unique_id(net.buses.iter().map(|b| b.id.as_str()), "barra");
        net.buses.push(Bus {
            id: id.clone(),
            kind: BusKind::Junction,
        });
        self.positions.insert(id.clone(), pos);
        self.canvas.selection = Some(Selection::Bus(id));
        self.revalidate();
    }

    /// Adiciona um ramo entre os dois primeiros barramentos (editável depois).
    fn add_branch(&mut self) {
        let Some(net) = self.net.as_mut() else {
            return;
        };
        if net.buses.len() < 2 {
            return;
        }
        let from = net.buses[0].id.clone();
        let to = net.buses[1].id.clone();
        let id = unique_id(net.branches.iter().filter_map(|b| b.id.as_deref()), "ramo");
        net.branches.push(Branch {
            id: Some(id),
            nodes: vec![from, to],
            element: Element::Line { consumers: 0 },
        });
        self.canvas.selection = Some(Selection::Branch(net.branches.len() - 1));
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.revalidate();
    }

    /// Exclui a seleção: um ramo, ou uma barra (em cascata com seus ramos).
    fn delete_selection(&mut self) {
        let Some(sel) = self.canvas.selection.take() else {
            return;
        };
        let Some(net) = self.net.as_mut() else {
            return;
        };
        match sel {
            Selection::Bus(id) => {
                net.buses.retain(|b| b.id != id);
                net.branches.retain(|b| !b.nodes.iter().any(|n| n == &id));
                self.positions.remove(&id);
                self.hidden_bus_labels.remove(&id);
            }
            Selection::Branch(i) => {
                if i < net.branches.len() {
                    net.branches.remove(i);
                }
            }
        }
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.revalidate();
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
            egui::ComboBox::from_id_salt("conjunto")
                .selected_text(texto)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.switch, String::new(), "Sistema inteiro");
                    for id in &switch_ids {
                        ui.selectable_value(&mut self.switch, id.clone(), id);
                    }
                });
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
            }
            if ui.button("ordenar por tempo").clicked() {
                self.scenario
                    .events
                    .sort_by(|a, b| a.at_min.total_cmp(&b.at_min));
            }
        });

        let mut remover: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("scroll_eventos")
            .max_height(280.0)
            .show(ui, |ui| {
                for (i, ev) in self.scenario.events.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("✕").clicked() {
                            remover = Some(i);
                        }
                        ui.add(
                            egui::DragValue::new(&mut ev.at_min)
                                .suffix(" min")
                                .range(0.0..=f64::MAX)
                                .speed(1.0),
                        );
                        egui::ComboBox::from_id_salt(("ev_bus", i))
                            .selected_text(ev.bus.clone())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for id in &bus_ids {
                                    ui.selectable_value(&mut ev.bus, id.clone(), id);
                                }
                            });
                        egui::ComboBox::from_id_salt(("ev_action", i))
                            .selected_text(action_label(ev.action))
                            .show_ui(ui, |ui| {
                                for a in
                                    [Action::Fault, Action::Repair, Action::Open, Action::Close]
                                {
                                    ui.selectable_value(&mut ev.action, a, action_label(a));
                                }
                            });
                    });
                }
            });
        if let Some(i) = remover {
            self.scenario.events.remove(i);
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
                ui.label(format!("Conjunto: {} — Cc = {} consumidores", r.alvo, r.cc));
                ui.horizontal(|ui| {
                    ui.heading(format!("DEC = {:.3} h", r.ind.dec_h));
                    ui.add_space(16.0);
                    ui.heading(format!("FEC = {:.3}", r.ind.fec));
                });
                ui.weak(format!("({:.1} min)", r.ind.dec_h * 60.0));
            }
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(230, 120, 120), format!("✗ {e}"));
            }
        }
    }
}

/// Centro geométrico das posições atuais (origem se vazio).
fn centroid(positions: &HashMap<String, Pos2>) -> Pos2 {
    if positions.is_empty() {
        return Pos2::ZERO;
    }
    let n = positions.len() as f32;
    let soma = positions
        .values()
        .fold(Vec2::ZERO, |acc, p| acc + p.to_vec2());
    (soma / n).to_pos2()
}

/// Primeiro id `{prefixo}{n}` (n≥1) que não colide com os existentes.
fn unique_id<'a>(existing: impl Iterator<Item = &'a str>, prefixo: &str) -> String {
    let usados: std::collections::HashSet<&str> = existing.collect();
    (1..)
        .map(|n| format!("{prefixo}{n}"))
        .find(|id| !usados.contains(id.as_str()))
        .expect("sequência infinita sempre acha um id livre")
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
