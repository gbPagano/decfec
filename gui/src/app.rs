//! Estado e laço de UI da aplicação.

use std::collections::HashMap;

use decfec::topology::Network;
use egui::Pos2;

use crate::canvas;
use crate::engine::{self, Report};

/// Rede de referência usada como conteúdo inicial dos editores (embutida em
/// tempo de compilação — funciona em WASM, sem filesystem).
const REDE_PADRAO: &str = include_str!("../../networks/ref-exercise.ron");
const CENARIO_PADRAO: &str = include_str!("../../scenarios/item_a.ron");

/// Aplicação egui.
///
/// Nesta etapa o fluxo é dirigido por texto RON (rede + cenário). O editor de
/// grafo em canvas entra nas próximas etapas, mas já reaproveitará `net`.
pub struct App {
    /// Texto RON da rede (editável).
    net_ron: String,
    /// Texto RON do cenário de faltas (editável).
    scenario_ron: String,
    /// Chave para o conjunto a jusante; vazio = sistema inteiro.
    switch: String,

    /// Rede carregada e validada (ou `None` se o último parse falhou).
    net: Option<Network>,
    /// Posições dos nós (coordenadas-mundo) — só na UI, derivadas no carregamento.
    positions: HashMap<String, Pos2>,
    /// Mensagem do último carregamento da rede (erro, ou resumo de sucesso).
    net_status: Result<String, String>,
    /// Resultado da última simulação.
    report: Option<Result<Report, String>>,
}

impl App {
    /// Constrói a aplicação a partir do contexto de criação do eframe.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            net_ron: REDE_PADRAO.to_string(),
            scenario_ron: CENARIO_PADRAO.to_string(),
            switch: "1".to_string(),
            net: None,
            positions: HashMap::new(),
            net_status: Ok(String::new()),
            report: None,
        };
        app.load_network();
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
        // Rede mudou: o resultado anterior não vale mais.
        self.report = None;
    }

    /// Roda a simulação com a rede carregada e o cenário/chave atuais.
    fn simulate(&mut self) {
        let switch = self.switch.trim();
        self.report = Some(match &self.net {
            Some(net) => engine::run(net, &self.scenario_ron, Some(switch)),
            None => Err("carregue uma rede válida antes de simular".to_string()),
        });
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
}

impl App {
    /// Painel esquerdo: editor RON da rede + carregamento/validação.
    fn painel_rede(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.strong("Rede (RON)");
        if ui.button("Carregar / validar").clicked() {
            self.load_network();
        }
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
        egui::ScrollArea::both()
            .id_salt("scroll_rede")
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.net_ron)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(28),
                );
            });
    }

    /// Painel central: o grafo da rede (somente leitura nesta etapa).
    fn painel_canvas(&mut self, ui: &mut egui::Ui) {
        match &self.net {
            Some(net) => canvas::draw(ui, net, &self.positions),
            None => {
                ui.centered_and_justified(|ui| {
                    ui.weak("Carregue uma rede válida (painel à esquerda) para vê-la aqui.");
                });
            }
        }
    }

    /// Painel direito: editor RON do cenário, seleção de conjunto e resultados.
    fn painel_cenario(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.strong("Cenário de faltas (RON)");
        ui.horizontal(|ui| {
            ui.label("Conjunto — chave a jusante:");
            ui.add(
                egui::TextEdit::singleline(&mut self.switch)
                    .hint_text("vazio = sistema inteiro")
                    .desired_width(120.0),
            );
            if ui.button("▶ Simular").clicked() {
                self.simulate();
            }
        });

        self.painel_resultado(ui);

        ui.separator();
        egui::ScrollArea::both()
            .id_salt("scroll_cenario")
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.scenario_ron)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(28),
                );
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
