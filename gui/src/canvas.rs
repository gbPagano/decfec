//! Desenho e interação do grafo da rede num canvas egui.
//!
//! As **coordenadas dos nós vivem só aqui/na UI** (um `HashMap<id, Pos2>` em
//! coordenadas-mundo) — o domínio `decfec` não tem geometria. Uma câmera
//! ([`CanvasState`]) com pan/zoom mapeia mundo→tela; assim arrastar um nó não
//! reescala o grafo inteiro (o que aconteceria com um fit-to-view por quadro).

use std::collections::{BTreeMap, HashMap, VecDeque};

use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use decfec::topology::{Branch, Element, Network, State};

/// Espaçamento entre camadas (eixo X) e entre nós de uma camada (eixo Y), em
/// coordenadas-mundo.
const DX: f32 = 180.0;
const DY: f32 = 80.0;
/// Raio do nó, em pixels de tela.
const NODE_R: f32 = 7.0;
/// Tolerância de clique (px) para nós e arestas.
const HIT_SLOP: f32 = 6.0;

/// O que está selecionado no canvas.
#[derive(Clone, PartialEq)]
pub enum Selection {
    /// Um barramento, por id.
    Bus(String),
    /// Um ramo, por índice em [`Network::branches`].
    Branch(usize),
}

/// Estado de câmera/interação do canvas (persiste entre quadros).
pub struct CanvasState {
    /// Deslocamento da câmera, em pixels de tela.
    pan: Vec2,
    /// Fator de zoom (px de tela por unidade-mundo).
    zoom: f32,
    /// Se `true`, reenquadra o grafo no próximo quadro.
    needs_fit: bool,
    /// O que o arrasto atual está movendo.
    drag: Drag,
    /// Seleção atual (lida pelo painel de edição).
    pub selection: Option<Selection>,
}

enum Drag {
    None,
    Pan,
    Node(String),
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            needs_fit: true,
            drag: Drag::None,
            selection: None,
        }
    }
}

impl CanvasState {
    /// Pede um reenquadramento (fit-to-view) no próximo desenho.
    pub fn request_fit(&mut self) {
        self.needs_fit = true;
    }
}

/// Gera um layout determinístico em camadas: a profundidade (X) é a distância
/// BFS até a fonte mais próxima; nós de mesma profundidade são empilhados em Y
/// na ordem de declaração. Cobre todos os barramentos (rede validada é conexa).
pub fn layout(net: &Network) -> HashMap<String, Pos2> {
    let adj = net.adjacency();

    // Profundidade BFS a partir de todas as fontes simultaneamente.
    let mut depth: HashMap<&str, i32> = HashMap::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    for s in net.sources() {
        depth.insert(s, 0);
        queue.push_back(s);
    }
    while let Some(u) = queue.pop_front() {
        let du = depth[u];
        if let Some(branches) = adj.get(u) {
            for &bi in branches {
                let b = &net.branches[bi];
                let v = if b.from == u {
                    b.to.as_str()
                } else {
                    b.from.as_str()
                };
                if !depth.contains_key(v) {
                    depth.insert(v, du + 1);
                    queue.push_back(v);
                }
            }
        }
    }

    // Agrupa por profundidade preservando a ordem de declaração (determinismo).
    let mut por_camada: BTreeMap<i32, Vec<&str>> = BTreeMap::new();
    for bus in &net.buses {
        let d = depth.get(bus.id.as_str()).copied().unwrap_or(0);
        por_camada.entry(d).or_default().push(bus.id.as_str());
    }

    let mut pos = HashMap::new();
    for (&d, ids) in &por_camada {
        let n = ids.len() as f32;
        for (i, &id) in ids.iter().enumerate() {
            let x = d as f32 * DX;
            let y = (i as f32 - (n - 1.0) / 2.0) * DY;
            pos.insert(id.to_string(), Pos2::new(x, y));
        }
    }
    pos
}

/// Desenha o grafo e processa interação (arrastar nós, pan, zoom, seleção).
///
/// `positions` é mutável porque arrastar um nó atualiza sua posição-mundo.
pub fn draw(
    ui: &mut egui::Ui,
    net: &Network,
    positions: &mut HashMap<String, Pos2>,
    st: &mut CanvasState,
) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = resp.rect;
    let center = rect.center();

    if st.needs_fit {
        if let Some((pan, zoom)) = fit(positions, rect) {
            st.pan = pan;
            st.zoom = zoom;
        }
        st.needs_fit = false;
    }

    // Câmera em variáveis locais; alterações (zoom/pan) gravam de volta no fim.
    let mut pan = st.pan;
    let mut zoom = st.zoom;
    let pointer = resp.hover_pos();

    // Zoom em torno do cursor.
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let p = pointer.unwrap_or(center);
            let w = to_world(center, pan, zoom, p);
            zoom = (zoom * (scroll * 0.0015).exp()).clamp(0.05, 20.0);
            pan = p - center - w.to_vec2() * zoom;
        }
    }

    // Início do arrasto: decide se move um nó ou faz pan.
    if resp.drag_started() {
        st.drag = match pointer.and_then(|p| node_at(positions, center, pan, zoom, p)) {
            Some(id) => {
                st.selection = Some(Selection::Bus(id.clone()));
                Drag::Node(id)
            }
            None => Drag::Pan,
        };
    }
    if resp.dragged() {
        match &st.drag {
            Drag::Pan => pan += resp.drag_delta(),
            Drag::Node(id) => {
                if let Some(w) = positions.get_mut(id) {
                    *w += resp.drag_delta() / zoom;
                }
            }
            Drag::None => {}
        }
    }
    if resp.drag_stopped() {
        st.drag = Drag::None;
    }

    // Clique simples: seleciona nó, ou aresta, ou limpa.
    if resp.clicked() {
        let p = pointer.unwrap_or(center);
        st.selection = node_at(positions, center, pan, zoom, p)
            .map(Selection::Bus)
            .or_else(|| edge_at(net, positions, center, pan, zoom, p).map(Selection::Branch));
    }

    paint(&painter, net, positions, st, center, pan, zoom);

    st.pan = pan;
    st.zoom = zoom;
}

/// Pinta arestas e nós com o estado de câmera/seleção já resolvido.
fn paint(
    painter: &egui::Painter,
    net: &Network,
    positions: &HashMap<String, Pos2>,
    st: &CanvasState,
    center: Pos2,
    pan: Vec2,
    zoom: f32,
) {
    // Arestas primeiro, para os nós ficarem por cima.
    for (i, b) in net.branches.iter().enumerate() {
        let (Some(&p_from), Some(&p_to)) = (positions.get(&b.from), positions.get(&b.to)) else {
            continue;
        };
        let a = to_screen(center, pan, zoom, p_from);
        let z = to_screen(center, pan, zoom, p_to);
        let (mut cor, mut largura, tracejada) = edge_style(&b.element);
        if st.selection == Some(Selection::Branch(i)) {
            cor = Color32::from_rgb(250, 240, 120);
            largura += 1.5;
        }

        if tracejada {
            painter.add(egui::Shape::dashed_line(
                &[a, z],
                Stroke::new(largura, cor),
                8.0,
                6.0,
            ));
        } else {
            painter.line_segment([a, z], Stroke::new(largura, cor));
        }

        painter.text(
            a.lerp(z, 0.5),
            egui::Align2::CENTER_CENTER,
            edge_label(b),
            FontId::proportional(11.0),
            cor,
        );
    }

    for bus in &net.buses {
        let Some(&p) = positions.get(&bus.id) else {
            continue;
        };
        let c = to_screen(center, pan, zoom, p);
        let (preenchimento, mut contorno) = if bus.is_source() {
            (Color32::from_rgb(80, 130, 230), Color32::WHITE)
        } else {
            (Color32::from_gray(70), Color32::from_gray(180))
        };
        let mut largura = 1.5;
        if st.selection == Some(Selection::Bus(bus.id.clone())) {
            contorno = Color32::from_rgb(250, 240, 120);
            largura = 3.0;
        }
        painter.circle(c, NODE_R, preenchimento, Stroke::new(largura, contorno));
        painter.text(
            c + Vec2::new(0.0, -NODE_R - 8.0),
            egui::Align2::CENTER_CENTER,
            &bus.id,
            FontId::proportional(11.0),
            contorno,
        );
    }
}

// --- transformações câmera ---

fn to_screen(center: Pos2, pan: Vec2, zoom: f32, w: Pos2) -> Pos2 {
    center + pan + w.to_vec2() * zoom
}

fn to_world(center: Pos2, pan: Vec2, zoom: f32, s: Pos2) -> Pos2 {
    ((s - center - pan) / zoom).to_pos2()
}

/// Barramento sob o ponto de tela `p` (o mais próximo dentro da tolerância).
fn node_at(
    positions: &HashMap<String, Pos2>,
    center: Pos2,
    pan: Vec2,
    zoom: f32,
    p: Pos2,
) -> Option<String> {
    positions
        .iter()
        .map(|(id, &w)| (id, (to_screen(center, pan, zoom, w) - p).length()))
        .filter(|&(_, d)| d <= NODE_R + HIT_SLOP)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id.clone())
}

/// Ramo sob o ponto de tela `p` (segmento mais próximo dentro da tolerância).
fn edge_at(
    net: &Network,
    positions: &HashMap<String, Pos2>,
    center: Pos2,
    pan: Vec2,
    zoom: f32,
    p: Pos2,
) -> Option<usize> {
    net.branches
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let a = to_screen(center, pan, zoom, *positions.get(&b.from)?);
            let z = to_screen(center, pan, zoom, *positions.get(&b.to)?);
            Some((i, dist_to_segment(p, a, z)))
        })
        .filter(|&(_, d)| d <= HIT_SLOP)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Distância de um ponto ao segmento `a`–`z`.
fn dist_to_segment(p: Pos2, a: Pos2, z: Pos2) -> f32 {
    let az = z - a;
    let len2 = az.length_sq();
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((p - a).dot(az) / len2).clamp(0.0, 1.0)
    };
    (a + az * t - p).length()
}

/// Câmera (pan, zoom) que enquadra todos os nós em `rect`, com margem e
/// preservando o aspecto. `None` se não há nós.
fn fit(positions: &HashMap<String, Pos2>, rect: Rect) -> Option<(Vec2, f32)> {
    let mut it = positions.values();
    let first = *it.next()?;
    let mut mundo = Rect::from_min_max(first, first);
    for &p in it {
        mundo.extend_with(p);
    }
    let mundo = mundo.expand(1.0);

    let avail = rect.shrink(40.0);
    let zoom = (avail.width() / mundo.width())
        .min(avail.height() / mundo.height())
        .clamp(0.05, 20.0);
    // Queremos to_screen(mundo.center()) == rect.center(): pan = -c_mundo * zoom.
    let pan = -mundo.center().to_vec2() * zoom;
    Some((pan, zoom))
}

// --- estilo das arestas ---

/// Cor, largura e se é tracejada, conforme o tipo de ramo.
fn edge_style(el: &Element) -> (Color32, f32, bool) {
    match el {
        // Chave NA (tie): laranja tracejado.
        Element::Switch {
            normal: State::Open,
        } => (Color32::from_rgb(230, 160, 60), 2.0, true),
        // Chave NF (seccionadora): verde.
        Element::Switch {
            normal: State::Closed,
        } => (Color32::from_rgb(110, 200, 110), 2.5, false),
        // Linha: cinza-claro; mais grossa se carrega consumidores.
        Element::Line { consumers } => {
            let l = if *consumers > 0 { 3.0 } else { 1.5 };
            (Color32::from_gray(170), l, false)
        }
    }
}

/// Rótulo de uma aresta: id da chave, ou "id (Nc)" para linhas com carga.
fn edge_label(b: &Branch) -> String {
    match b.element {
        Element::Line { consumers } if consumers > 0 => {
            format!("{} ({}c)", b.label(), consumers)
        }
        _ => b.label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REDE: &str = include_str!("../../networks/ref-exercise.ron");

    #[test]
    fn layout_cobre_todos_os_barramentos() {
        let net = Network::from_ron(REDE).unwrap();
        let pos = layout(&net);
        for bus in &net.buses {
            assert!(pos.contains_key(&bus.id), "sem posição para '{}'", bus.id);
        }
    }
}
