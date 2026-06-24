//! Estado e laço de UI da aplicação.

/// Aplicação egui. Por enquanto apenas um esqueleto; o editor de grafo e os
/// painéis de cenário/resultados são adicionados nas etapas seguintes.
pub struct App {}

impl App {
    /// Constrói a aplicação a partir do contexto de criação do eframe.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {}
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("decfec — editor de redes e falhas");
            ui.label("Esqueleto inicial (eframe + WASM).");
        });
    }
}
