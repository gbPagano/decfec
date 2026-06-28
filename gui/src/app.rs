//! Estado e laço de UI da aplicação.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RonKind {
    Network,
    Scenario,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RonMode {
    Import,
    Export,
}

struct RonDialog {
    kind: RonKind,
    mode: RonMode,
    text: String,
    status: Option<Result<String, String>>,
}

enum FileResult {
    Uploaded {
        kind: RonKind,
        text: String,
        name: String,
    },
    Downloaded {
        kind: RonKind,
        name: String,
    },
    Error {
        kind: RonKind,
        message: String,
    },
}

type PendingFileResult = Rc<RefCell<Option<FileResult>>>;

#[derive(Clone)]
struct GraphClipboard {
    buses: Vec<CopiedBus>,
    branches: Vec<Branch>,
}

#[derive(Clone)]
struct CopiedBus {
    bus: Bus,
    pos: Pos2,
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
    /// Janela modal de import/export RON aberta, se houver.
    ron_dialog: Option<RonDialog>,
    /// Se `true`, exibe a janela de ajuda da aplicação.
    help_open: bool,
    /// Reposiciona a ajuda no centro apenas no primeiro frame da abertura.
    help_center_on_open: bool,
    /// Resultado assíncrono de upload/download em WASM.
    pending_file_result: PendingFileResult,
    /// Clipboard interno para Ctrl+C/Ctrl+V no grafo.
    graph_clipboard: Option<GraphClipboard>,
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
            ron_dialog: None,
            help_open: false,
            help_center_on_open: false,
            pending_file_result: Rc::new(RefCell::new(None)),
            graph_clipboard: None,
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

    fn ron_text(&self, kind: RonKind) -> String {
        match kind {
            RonKind::Network => self
                .network_document_to_ron()
                .unwrap_or_else(|_| self.net_ron.clone()),
            RonKind::Scenario => engine::scenario_to_ron(&self.scenario),
        }
    }

    fn open_ron_dialog(&mut self, kind: RonKind, mode: RonMode) {
        self.ron_dialog = Some(RonDialog {
            kind,
            mode,
            text: self.ron_text(kind),
            status: None,
        });
    }

    fn ron_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.ron_dialog else {
            return;
        };

        let mut open = true;
        let mut action = None;
        egui::Window::new(dialog_title(dialog.kind, dialog.mode))
            .collapsible(false)
            .resizable(true)
            .default_width(680.0)
            .default_height(520.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| match dialog.mode {
                    RonMode::Import => {
                        if ui.button("Upload de arquivo").clicked() {
                            action = Some(RonDialogAction::Upload(dialog.kind));
                        }
                        if ui.button("Importar texto").clicked() {
                            action = Some(RonDialogAction::ImportText {
                                kind: dialog.kind,
                                text: dialog.text.clone(),
                            });
                        }
                    }
                    RonMode::Export => {
                        if ui.button("Download de arquivo").clicked() {
                            action = Some(RonDialogAction::Download {
                                kind: dialog.kind,
                                text: dialog.text.clone(),
                            });
                        }
                    }
                });
                if let Some(status) = &dialog.status {
                    match status {
                        Ok(msg) => ui.colored_label(egui::Color32::from_rgb(120, 200, 120), msg),
                        Err(e) => ui.colored_label(egui::Color32::from_rgb(230, 120, 120), e),
                    };
                }
                ui.add_space(4.0);
                egui::ScrollArea::both()
                    .id_salt(("ron_dialog_scroll", dialog.kind, dialog.mode))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut dialog.text)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(22),
                        );
                    });
            });

        if !open {
            self.ron_dialog = None;
            return;
        }
        if let Some(action) = action {
            self.handle_ron_dialog_action(ctx, action);
        }
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        if !self.help_open {
            return;
        }

        let mut window = egui::Window::new("Ajuda")
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .default_height(520.0);
        if self.help_center_on_open {
            window = window.anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO);
            self.help_center_on_open = false;
        }

        window.open(&mut self.help_open).show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("help_window_scroll")
                    .show(ui, |ui| {
                        ui.heading("Como usar o programa");
                        ui.label(
                            "Monte ou importe uma rede, descreva os eventos do cenário e clique em Simular para calcular DEC/FEC do sistema inteiro ou de um conjunto a jusante.",
                        );

                        ui.separator();
                        ui.strong("Rede e grafo");
                        ui.label("- Use + Barra para criar um novo barramento no centro da vista.");
                        ui.label("- Use + Ramo para criar uma ligação entre barras.");
                        ui.label(
                            "- Se houver duas ou mais barras selecionadas, + Ramo conecta as selecionadas.",
                        );
                        ui.label(
                            "- Se não houver duas barras selecionadas, + Ramo conecta automaticamente as duas últimas barras criadas.",
                        );
                        ui.label("- Arraste uma barra para reposicionar no canvas.");
                        ui.label("- Arraste o fundo para mover a vista e use a roda do mouse para zoom.");
                        ui.label("- Ajustar à vista reenquadra a rede; Auto-layout reorganiza as barras.");

                        ui.add_space(6.0);
                        ui.strong("Seleção e edição");
                        ui.label("- Clique em uma barra ou ramo para editar no painel Rede.");
                        ui.label(
                            "- Segure Ctrl enquanto clica para selecionar várias barras ou alternar itens na seleção.",
                        );
                        ui.label("- Use Ctrl+C e Ctrl+V para copiar e colar a seleção no grafo.");
                        ui.label("- Use Delete ou Excluir seleção para remover itens selecionados.");
                        ui.label(
                            "- Para esconder o label de uma barra, selecione a barra e desmarque Exibir label no grafo.",
                        );
                        ui.label(
                            "- Barras podem ser Subestação, Junção ou Chave. Para chaves, escolha NF ou NA/tie.",
                        );
                        ui.label(
                            "- Ramos representam trechos de linha e guardam o número de consumidores daquele bloco.",
                        );

                        ui.add_space(6.0);
                        ui.strong("Cenário e simulação");
                        ui.label(
                            "- No painel da direita, escolha Sistema inteiro ou um conjunto a jusante de uma chave.",
                        );
                        ui.label("- Adicione eventos com tempo em minutos, barra e ação.");
                        ui.label("- Fault cria a falta, Repair remove a falta, Open abre uma chave e Close fecha.");
                        ui.label(
                            "- Para transferir carga, normalmente use Open na chave de seccionamento e Close na chave NA/tie.",
                        );
                        ui.label("- Ordenar por tempo ajuda a revisar a sequência antes de simular.");

                        ui.add_space(6.0);
                        ui.strong("Importar e exportar");
                        ui.label(
                            "- Rede > Importar/Exportar trabalha com a rede e o layout do canvas em RON.",
                        );
                        ui.label(
                            "- Eventos > Importar/Exportar trabalha apenas com o cenário de eventos em RON.",
                        );
                        ui.label(
                            "- No navegador, use Upload de arquivo e Download de arquivo dentro das janelas de import/export.",
                        );
                        ui.label(
                            "- O estado editado graficamente é a fonte da verdade. Exporte para gerar o RON atualizado.",
                        );

                        ui.add_space(6.0);
                        ui.strong("Dicas");
                        ui.label(
                            "- Se uma simulação der erro, confira ids duplicados, ramos sem nós válidos e eventos apontando para barras existentes.",
                        );
                        ui.label(
                            "- Interrupções menores que 3 minutos são tratadas como momentâneas e não entram no DEC/FEC.",
                        );
                    });
        });
    }

    fn handle_ron_dialog_action(&mut self, ctx: &egui::Context, action: RonDialogAction) {
        match action {
            RonDialogAction::ImportText { kind, text } => {
                let status = self.import_ron_text(kind, text);
                if let Some(dialog) = &mut self.ron_dialog {
                    dialog.status = Some(status);
                }
            }
            RonDialogAction::Upload(kind) => {
                pick_ron_file(kind, self.pending_file_result.clone(), ctx.clone())
            }
            RonDialogAction::Download { kind, text } => {
                save_ron_file(kind, text, self.pending_file_result.clone(), ctx.clone())
            }
        }
    }

    fn import_ron_text(&mut self, kind: RonKind, text: String) -> Result<String, String> {
        match kind {
            RonKind::Network => {
                self.net_ron = text;
                self.load_network();
                self.mark_network_dirty();
                match &self.net_status {
                    Ok(msg) => Ok(format!("rede importada: {msg}")),
                    Err(e) => Err(e.clone()),
                }
            }
            RonKind::Scenario => match engine::load_scenario(&text) {
                Ok(scenario) => {
                    self.scenario = scenario;
                    self.scenario_ron = text;
                    self.report = None;
                    self.mark_scenario_dirty();
                    Ok(format!("{} eventos importados", self.scenario.events.len()))
                }
                Err(e) => {
                    self.report = Some(Err(e.clone()));
                    Err(e)
                }
            },
        }
    }

    fn handle_pending_file_result(&mut self) {
        let Some(result) = self.pending_file_result.borrow_mut().take() else {
            return;
        };

        match result {
            FileResult::Uploaded { kind, text, name } => {
                let status = self
                    .import_ron_text(kind, text.clone())
                    .map(|msg| format!("arquivo '{name}' importado: {msg}"))
                    .map_err(|e| format!("arquivo '{name}' carregado, mas não importado: {e}"));
                if !matches!(self.ron_dialog.as_ref(), Some(dialog) if dialog.kind == kind) {
                    self.open_ron_dialog(kind, RonMode::Import);
                }
                if let Some(dialog) = &mut self.ron_dialog {
                    dialog.kind = kind;
                    dialog.mode = RonMode::Import;
                    dialog.text = text;
                    dialog.status = Some(status);
                }
            }
            FileResult::Downloaded { kind, name } => {
                if let Some(dialog) = &mut self.ron_dialog
                    && dialog.kind == kind
                {
                    dialog.status = Some(Ok(format!("arquivo '{name}' salvo")));
                }
            }
            FileResult::Error { kind, message } => {
                if let Some(dialog) = &mut self.ron_dialog
                    && dialog.kind == kind
                {
                    dialog.status = Some(Err(message));
                }
            }
        }
    }
}

enum RonDialogAction {
    ImportText { kind: RonKind, text: String },
    Upload(RonKind),
    Download { kind: RonKind, text: String },
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("titulo").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("DEC/FEC: Calculadora de Indicadores");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Ajuda").clicked() {
                        self.help_open = true;
                        self.help_center_on_open = true;
                    }
                });
            });
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

        self.handle_pending_file_result();
        self.ron_dialog(ui.ctx());
        self.help_window(ui.ctx());
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
            if ui.button("Importar").clicked() {
                self.open_ron_dialog(RonKind::Network, RonMode::Import);
            }
            if ui.button("Exportar").clicked() {
                self.open_ron_dialog(RonKind::Network, RonMode::Export);
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
                    if !new_id.is_empty()
                        && new_id != id
                        && !bus_id_already_exists(net, &id, new_id)
                    {
                        rename_to = Some(new_id.to_string());
                    }
                }

                let edited_id = id_buf.trim().to_string();
                if !edited_id.is_empty()
                    && edited_id != id
                    && bus_id_already_exists(net, &id, &edited_id)
                {
                    let msg = format!("já existe uma barra com id '{edited_id}'");
                    ui.colored_label(egui::Color32::from_rgb(230, 120, 120), &msg);
                    self.net_status = Err(msg);
                    return;
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

        if !ui.ctx().egui_wants_keyboard_input() {
            let copy_pressed = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::C));
            if copy_pressed {
                self.copy_selection();
            }

            let paste_pressed = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::V));
            if paste_pressed {
                self.paste_selection();
            }

            let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete));
            if delete_pressed && !self.canvas.selections.is_empty() {
                self.delete_selection();
            }
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

    fn copy_selection(&mut self) {
        let Some(net) = &self.net else {
            return;
        };
        let Some(clipboard) = copy_graph_selection(net, &self.positions, &self.canvas.selections)
        else {
            return;
        };
        self.graph_clipboard = Some(clipboard);
        self.net_status = Ok("seleção copiada".to_string());
    }

    fn paste_selection(&mut self) {
        let Some(net) = self.net.as_mut() else {
            return;
        };
        let Some(clipboard) = self.graph_clipboard.as_mut() else {
            return;
        };

        let paste_pos = self.canvas.paste_pos();
        let selections = paste_graph_clipboard(net, &mut self.positions, clipboard, paste_pos);
        if selections.is_empty() {
            return;
        }

        self.canvas.selections = selections;
        self.branch_nodes_editor = None;
        self.bus_id_editor = None;
        self.report = None;
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
            if ui.button("Importar").clicked() {
                self.open_ron_dialog(RonKind::Scenario, RonMode::Import);
            }
            if ui.button("Exportar").clicked() {
                self.open_ron_dialog(RonKind::Scenario, RonMode::Export);
            }
        });
        ui.horizontal(|ui| {
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

fn unique_suffixed_id(used: &HashSet<String>, base: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{base}_copy")
            } else {
                format!("{base}_copy{n}")
            }
        })
        .find(|id| !used.contains(id))
        .expect("sequência infinita sempre acha um id livre")
}

fn bus_id_already_exists(net: &Network, current_id: &str, candidate: &str) -> bool {
    net.buses
        .iter()
        .any(|bus| bus.id != current_id && bus.id == candidate)
}

fn copy_graph_selection(
    net: &Network,
    positions: &HashMap<String, Pos2>,
    selections: &[Selection],
) -> Option<GraphClipboard> {
    let selected_bus_ids: HashSet<String> = selections
        .iter()
        .filter_map(|selection| match selection {
            Selection::Bus(id) => Some(id.clone()),
            Selection::Branch(_) => None,
        })
        .collect();
    let selected_branch_indices: HashSet<usize> = selections
        .iter()
        .filter_map(|selection| match selection {
            Selection::Branch(i) => Some(*i),
            Selection::Bus(_) => None,
        })
        .collect();

    let mut branch_indices = Vec::new();
    for (i, branch) in net.branches.iter().enumerate() {
        let explicitly_selected = selected_branch_indices.contains(&i);
        let connects_selected_buses = !selected_bus_ids.is_empty()
            && branch
                .nodes
                .iter()
                .all(|node| selected_bus_ids.contains(node));
        if explicitly_selected || connects_selected_buses {
            branch_indices.push(i);
        }
    }

    let mut bus_ids = selected_bus_ids;
    for &i in &branch_indices {
        if let Some(branch) = net.branches.get(i) {
            bus_ids.extend(branch.nodes.iter().cloned());
        }
    }

    let buses: Vec<CopiedBus> = net
        .buses
        .iter()
        .filter(|bus| bus_ids.contains(&bus.id))
        .map(|bus| CopiedBus {
            bus: bus.clone(),
            pos: positions.get(&bus.id).copied().unwrap_or(Pos2::ZERO),
        })
        .collect();
    if buses.is_empty() {
        return None;
    }

    let branches = branch_indices
        .into_iter()
        .filter_map(|i| net.branches.get(i).cloned())
        .collect();

    Some(GraphClipboard { buses, branches })
}

fn paste_graph_clipboard(
    net: &mut Network,
    positions: &mut HashMap<String, Pos2>,
    clipboard: &mut GraphClipboard,
    target_pos: Pos2,
) -> Vec<Selection> {
    if clipboard.buses.is_empty() {
        return Vec::new();
    }

    let offset = target_pos - copied_buses_center(&clipboard.buses);
    let mut used_bus_ids: HashSet<String> = net.buses.iter().map(|bus| bus.id.clone()).collect();
    let mut id_map = HashMap::new();
    let mut selections = Vec::new();

    for copied in &clipboard.buses {
        let new_id = unique_suffixed_id(&used_bus_ids, &copied.bus.id);
        used_bus_ids.insert(new_id.clone());
        id_map.insert(copied.bus.id.clone(), new_id.clone());

        let mut bus = copied.bus.clone();
        bus.id = new_id.clone();
        net.buses.push(bus);
        positions.insert(new_id.clone(), copied.pos + offset);
        selections.push(Selection::Bus(new_id));
    }

    let mut used_branch_ids: HashSet<String> = net
        .branches
        .iter()
        .filter_map(|branch| branch.id.clone())
        .collect();
    used_branch_ids.extend(used_bus_ids);
    for copied in &clipboard.branches {
        let Some(nodes) = copied
            .nodes
            .iter()
            .map(|node| id_map.get(node).cloned())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };

        let mut branch = copied.clone();
        branch.nodes = nodes;
        if let Some(id) = &copied.id {
            let new_id = unique_suffixed_id(&used_branch_ids, id);
            used_branch_ids.insert(new_id.clone());
            branch.id = Some(new_id);
        }

        let new_idx = net.branches.len();
        net.branches.push(branch);
        selections.push(Selection::Branch(new_idx));
    }

    selections
}

fn copied_buses_center(buses: &[CopiedBus]) -> Pos2 {
    let sum = buses
        .iter()
        .fold(egui::Vec2::ZERO, |acc, copied| acc + copied.pos.to_vec2());
    (sum / buses.len() as f32).to_pos2()
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

fn dialog_title(kind: RonKind, mode: RonMode) -> &'static str {
    match (kind, mode) {
        (RonKind::Network, RonMode::Import) => "Importar rede",
        (RonKind::Network, RonMode::Export) => "Exportar rede",
        (RonKind::Scenario, RonMode::Import) => "Importar eventos",
        (RonKind::Scenario, RonMode::Export) => "Exportar eventos",
    }
}

fn default_file_name(kind: RonKind) -> &'static str {
    match kind {
        RonKind::Network => "rede-layout.ron",
        RonKind::Scenario => "eventos.ron",
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_ron_file(kind: RonKind, pending: PendingFileResult, ctx: egui::Context) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("RON", &["ron"])
        .pick_file()
    else {
        return;
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("arquivo.ron")
        .to_string();
    let result = match std::fs::read_to_string(&path) {
        Ok(text) => FileResult::Uploaded { kind, text, name },
        Err(e) => FileResult::Error {
            kind,
            message: format!("erro ao ler arquivo: {e}"),
        },
    };
    *pending.borrow_mut() = Some(result);
    ctx.request_repaint();
}

#[cfg(target_arch = "wasm32")]
fn pick_ron_file(kind: RonKind, pending: PendingFileResult, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("RON", &["ron"])
            .pick_file()
            .await
        else {
            return;
        };
        let name = file.file_name();
        let result = match String::from_utf8(file.read().await) {
            Ok(text) => FileResult::Uploaded { kind, text, name },
            Err(e) => FileResult::Error {
                kind,
                message: format!("arquivo não é UTF-8 válido: {e}"),
            },
        };
        *pending.borrow_mut() = Some(result);
        ctx.request_repaint();
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn save_ron_file(kind: RonKind, text: String, pending: PendingFileResult, ctx: egui::Context) {
    let path = match std::env::current_dir() {
        Ok(dir) => dir.join(default_file_name(kind)),
        Err(e) => {
            *pending.borrow_mut() = Some(FileResult::Error {
                kind,
                message: format!("erro ao localizar diretório atual: {e}"),
            });
            ctx.request_repaint();
            return;
        }
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(default_file_name(kind))
        .to_string();
    let result = match std::fs::write(&path, text) {
        Ok(()) => FileResult::Downloaded { kind, name },
        Err(e) => FileResult::Error {
            kind,
            message: format!("erro ao salvar arquivo: {e}"),
        },
    };
    *pending.borrow_mut() = Some(result);
    ctx.request_repaint();
}

#[cfg(target_arch = "wasm32")]
fn save_ron_file(kind: RonKind, text: String, pending: PendingFileResult, ctx: egui::Context) {
    let name = default_file_name(kind).to_string();
    let result = match download_ron_file(&name, &text) {
        Ok(()) => FileResult::Downloaded { kind, name },
        Err(e) => FileResult::Error { kind, message: e },
    };
    *pending.borrow_mut() = Some(result);
    ctx.request_repaint();
}

#[cfg(target_arch = "wasm32")]
fn download_ron_file(name: &str, text: &str) -> Result<(), String> {
    use eframe::wasm_bindgen::{JsCast as _, JsValue};

    let window = web_sys::window().ok_or_else(|| "sem objeto window".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "sem objeto document".to_string())?;
    let body = document
        .body()
        .ok_or_else(|| "documento sem body".to_string())?;

    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(text));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("text/plain;charset=utf-8");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)
        .map_err(|e| format!("erro ao montar arquivo: {e:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| format!("erro ao criar download: {e:?}"))?;

    let anchor = document
        .create_element("a")
        .map_err(|e| format!("erro ao criar link de download: {e:?}"))?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "elemento de download inválido".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(name);
    body.append_child(&anchor)
        .map_err(|e| format!("erro ao anexar link de download: {e:?}"))?;
    anchor.click();
    body.remove_child(&anchor)
        .map_err(|e| format!("erro ao remover link de download: {e:?}"))?;
    web_sys::Url::revoke_object_url(&url)
        .map_err(|e| format!("erro ao liberar download: {e:?}"))?;
    Ok(())
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

    #[test]
    fn detecta_id_de_barra_duplicado_ignorando_barra_atual() {
        let net = Network {
            buses: vec![
                Bus {
                    id: "b1".to_string(),
                    kind: BusKind::Junction,
                },
                Bus {
                    id: "b2".to_string(),
                    kind: BusKind::Junction,
                },
            ],
            branches: Vec::new(),
        };

        assert!(bus_id_already_exists(&net, "b2", "b1"));
        assert!(!bus_id_already_exists(&net, "b2", "b2"));
        assert!(!bus_id_already_exists(&net, "b2", "b3"));
    }

    #[test]
    fn copia_e_cola_barras_com_ramo_entre_selecionadas() {
        let mut net = Network {
            buses: vec![
                Bus {
                    id: "s".to_string(),
                    kind: BusKind::Substation,
                },
                Bus {
                    id: "b1".to_string(),
                    kind: BusKind::Junction,
                },
            ],
            branches: vec![Branch {
                id: Some("tr_s_b1".to_string()),
                nodes: vec!["s".to_string(), "b1".to_string()],
                element: Element::Line { consumers: 10 },
            }],
        };
        let mut positions = HashMap::from([
            ("s".to_string(), Pos2::new(0.0, 0.0)),
            ("b1".to_string(), Pos2::new(100.0, 0.0)),
        ]);
        let selections = vec![
            Selection::Bus("s".to_string()),
            Selection::Bus("b1".to_string()),
        ];
        let mut clipboard =
            copy_graph_selection(&net, &positions, &selections).expect("seleção deve ser copiável");

        let pasted = paste_graph_clipboard(
            &mut net,
            &mut positions,
            &mut clipboard,
            Pos2::new(200.0, 50.0),
        );

        assert_eq!(net.buses[2].id, "s_copy");
        assert_eq!(net.buses[3].id, "b1_copy");
        assert_eq!(net.branches[1].id.as_deref(), Some("tr_s_b1_copy"));
        assert_eq!(
            net.branches[1].nodes,
            vec!["s_copy".to_string(), "b1_copy".to_string()]
        );
        assert_eq!(positions["s_copy"], Pos2::new(150.0, 50.0));
        assert_eq!(positions["b1_copy"], Pos2::new(250.0, 50.0));
        assert!(
            pasted
                .iter()
                .any(|selection| matches!(selection, Selection::Branch(1)))
        );

        paste_graph_clipboard(
            &mut net,
            &mut positions,
            &mut clipboard,
            Pos2::new(300.0, 80.0),
        );

        assert_eq!(net.buses[4].id, "s_copy2");
        assert_eq!(net.buses[5].id, "b1_copy2");
        assert_eq!(net.branches[2].id.as_deref(), Some("tr_s_b1_copy2"));
        assert_eq!(positions["s_copy2"], Pos2::new(250.0, 80.0));
        assert_eq!(positions["b1_copy2"], Pos2::new(350.0, 80.0));
    }

    #[test]
    fn copia_de_ramo_selecionado_inclui_barras_terminais() {
        let net = Network {
            buses: vec![
                Bus {
                    id: "a".to_string(),
                    kind: BusKind::Substation,
                },
                Bus {
                    id: "b".to_string(),
                    kind: BusKind::Junction,
                },
            ],
            branches: vec![Branch {
                id: Some("ramo".to_string()),
                nodes: vec!["a".to_string(), "b".to_string()],
                element: Element::Line { consumers: 0 },
            }],
        };
        let positions = HashMap::new();

        let clipboard = copy_graph_selection(&net, &positions, &[Selection::Branch(0)])
            .expect("ramo selecionado deve copiar seus terminais");

        assert_eq!(clipboard.buses.len(), 2);
        assert_eq!(clipboard.branches.len(), 1);
    }
}
