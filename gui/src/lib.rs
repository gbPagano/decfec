//! Interface gráfica (egui/eframe) do `decfec`, compilável para WebAssembly.
//!
//! Esta crate é apenas a camada de UI: todo o domínio (topologia e faltas) vem
//! da crate [`decfec`], consumida diretamente. Não há I/O de filesystem aqui —
//! redes e cenários entram/saem como texto RON, o que torna tudo WASM-friendly.

mod app;
mod canvas;
mod engine;

pub use app::App;
