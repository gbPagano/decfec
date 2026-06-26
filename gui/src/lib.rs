//! Interface gráfica (egui/eframe) do `decfec`, compilável para WebAssembly.
//!
//! Esta crate é apenas a camada de UI: todo o domínio (topologia e faltas) vem
//! da crate [`decfec`], consumida diretamente. Redes e cenários entram/saem como
//! texto RON; a UI só oferece diálogos opcionais para importar/exportar arquivos.

mod app;
mod canvas;
mod engine;

pub use app::App;
