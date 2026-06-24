//! Pontos de entrada: nativo (janela desktop) e WebAssembly (canvas no navegador).
//!
//! O nativo serve para iterar rápido sem a toolchain WASM; o web é a entrega
//! final via `trunk`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Entrada nativa: abre uma janela com a aplicação.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "decfec",
        native_options,
        Box::new(|cc| Ok(Box::new(decfec_gui::App::new(cc)))),
    )
}

/// Entrada web: anexa a aplicação ao `<canvas id="the_canvas_id">` da página.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("sem objeto window")
            .document()
            .expect("sem document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("não achei #the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#the_canvas_id não é um canvas");

        let result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(decfec_gui::App::new(cc)))),
            )
            .await;

        if let Err(e) = result {
            web_sys::console::error_1(&format!("falha ao iniciar eframe: {e:?}").into());
        }
    });
}
