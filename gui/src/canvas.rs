//! Desenho e interação do grafo da rede num canvas egui.
//!
//! As **coordenadas dos nós vivem só aqui/na UI** (um `HashMap<id, Pos2>` em
//! coordenadas-mundo) — o domínio `decfec` não tem geometria. Uma câmera
//! ([`CanvasState`]) com pan/zoom mapeia mundo→tela; assim arrastar um nó não
//! reescala o grafo inteiro (o que aconteceria com um fit-to-view por quadro).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use decfec::topology::{BusKind, Element, Network, State};

/// Espaçamento entre camadas (eixo X) e entre nós de uma camada (eixo Y), em
/// coordenadas-mundo.
const DX: f32 = 180.0;
const DY: f32 = 80.0;
/// Raio do nó, em pixels de tela.
const NODE_R: f32 = 7.0;
/// Tolerância de clique (px) para nós e arestas.
const HIT_SLOP: f32 = 6.0;
/// Distância do rótulo do ramo até a linha desenhada, em pixels de tela.
const EDGE_LABEL_OFFSET: f32 = 14.0;
/// Rótulos de ramos multiterminais precisam sair mais do centro do entroncamento.
const MULTITERMINAL_EDGE_LABEL_OFFSET: f32 = 32.0;
/// Distância do rótulo do barramento até o nó, em pixels de tela.
const BUS_LABEL_OFFSET: f32 = 14.0;
/// Deslocamento abstrato usado só para escolher lados em coordenadas-mundo.
const LABEL_DIRECTION_SCORE_OFFSET: f32 = 1.0;

/// O que está selecionado no canvas.
#[derive(Clone, PartialEq, Eq)]
pub enum Selection {
    /// Um barramento, por id.
    Bus(String),
    /// Um ramo, por índice em [`Network::branches`].
    Branch(usize),
}

struct BranchVisual {
    world_segments: Vec<(Pos2, Pos2)>,
    world_label_pos: Option<Pos2>,
}

#[derive(Clone, Copy)]
struct Camera {
    center: Pos2,
    pan: Vec2,
    zoom: f32,
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
    /// Centro visível atual, em coordenadas-mundo.
    view_center_world: Pos2,
    /// Seleções atuais (lidas pelo painel de edição).
    pub selections: Vec<Selection>,
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
            view_center_world: Pos2::ZERO,
            selections: Vec::new(),
        }
    }
}

impl CanvasState {
    /// Pede um reenquadramento (fit-to-view) no próximo desenho.
    pub fn request_fit(&mut self) {
        self.needs_fit = true;
    }

    pub fn set_selection(&mut self, selection: Selection) {
        self.selections.clear();
        self.selections.push(selection);
    }

    pub fn clear_selection(&mut self) {
        self.selections.clear();
    }

    /// Posição para criar um nó novo: centro visível atual do canvas.
    pub fn insertion_pos(&self) -> Pos2 {
        self.view_center_world
    }

    fn is_selected(&self, selection: &Selection) -> bool {
        self.selections.contains(selection)
    }

    fn toggle_selection(&mut self, selection: Selection) {
        if let Some(i) = self.selections.iter().position(|sel| sel == &selection) {
            self.selections.remove(i);
        } else {
            self.selections.push(selection);
        }
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
                for v in &b.nodes {
                    if v == u {
                        continue;
                    }
                    let v = v.as_str();
                    if !depth.contains_key(v) {
                        depth.insert(v, du + 1);
                        queue.push_back(v);
                    }
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
    hidden_bus_labels: &HashSet<String>,
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
                let selection = Selection::Bus(id.clone());
                let ctrl = ui.input(|i| i.modifiers.ctrl);
                if !ctrl && !st.is_selected(&selection) {
                    st.set_selection(selection);
                }
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
        let selection = node_at(positions, center, pan, zoom, p)
            .map(Selection::Bus)
            .or_else(|| edge_at(net, positions, center, pan, zoom, p).map(Selection::Branch));
        if ui.input(|i| i.modifiers.ctrl) {
            if let Some(selection) = selection {
                st.toggle_selection(selection);
            }
        } else if let Some(selection) = selection {
            st.set_selection(selection);
        } else {
            st.clear_selection();
        }
    }

    st.view_center_world = to_world(center, pan, zoom, center);

    paint(
        &painter,
        net,
        positions,
        st,
        hidden_bus_labels,
        Camera { center, pan, zoom },
    );

    st.pan = pan;
    st.zoom = zoom;
}

/// Pinta arestas e nós com o estado de câmera/seleção já resolvido.
fn paint(
    painter: &egui::Painter,
    net: &Network,
    positions: &HashMap<String, Pos2>,
    st: &CanvasState,
    hidden_bus_labels: &HashSet<String>,
    camera: Camera,
) {
    let mut branch_visuals = Vec::new();

    // Arestas primeiro, para os nós ficarem por cima.
    for (i, b) in net.branches.iter().enumerate() {
        let points: Vec<Pos2> = b
            .nodes
            .iter()
            .filter_map(|node| positions.get(node).copied())
            .collect();
        if points.len() < 2 {
            continue;
        }
        let screen_points: Vec<Pos2> = points
            .iter()
            .map(|&p| to_screen(camera.center, camera.pan, camera.zoom, p))
            .collect();
        let world_branch_center = midpoint(&points);
        let branch_center = midpoint(&screen_points);
        let label_dir = edge_label_direction(&points);
        let label_pos = edge_label_pos(&screen_points, label_dir);
        let label = edge_label(&b.element);
        let has_label = label.is_some();
        let segments: Vec<(Pos2, Pos2)> = if screen_points.len() == 2 {
            vec![(screen_points[0], screen_points[1])]
        } else {
            screen_points.iter().map(|&p| (branch_center, p)).collect()
        };
        let world_segments: Vec<(Pos2, Pos2)> = if points.len() == 2 {
            vec![(points[0], points[1])]
        } else {
            points.iter().map(|&p| (world_branch_center, p)).collect()
        };
        let (mut cor, mut largura, tracejada) = edge_style(&b.element);
        if st.is_selected(&Selection::Branch(i)) {
            cor = Color32::from_rgb(250, 240, 120);
            largura += 1.5;
        }

        for &(a, z) in &segments {
            paint_segment(painter, a, z, largura, cor, tracejada);
        }

        if let Some(label) = label {
            painter.text(
                label_pos,
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(11.0),
                cor,
            );
        }
        branch_visuals.push(BranchVisual {
            world_segments,
            world_label_pos: has_label
                .then_some(world_branch_center + label_dir * LABEL_DIRECTION_SCORE_OFFSET),
        });
    }

    for bus in &net.buses {
        let Some(&p) = positions.get(&bus.id) else {
            continue;
        };
        let c = to_screen(camera.center, camera.pan, camera.zoom, p);
        let (preenchimento, mut contorno) = match bus.kind {
            BusKind::Substation => (Color32::from_rgb(80, 130, 230), Color32::WHITE),
            BusKind::Junction => (Color32::from_gray(70), Color32::from_gray(180)),
            BusKind::Switch {
                normal: State::Closed,
            } => (
                Color32::from_rgb(70, 110, 70),
                Color32::from_rgb(130, 230, 130),
            ),
            BusKind::Switch {
                normal: State::Open,
            } => (
                Color32::from_rgb(110, 80, 45),
                Color32::from_rgb(240, 170, 70),
            ),
        };
        let mut largura = 1.5;
        if st.is_selected(&Selection::Bus(bus.id.clone())) {
            contorno = Color32::from_rgb(250, 240, 120);
            largura = 3.0;
        }
        painter.circle(c, NODE_R, preenchimento, Stroke::new(largura, contorno));
        if hidden_bus_labels.contains(&bus.id) {
            continue;
        }
        painter.text(
            bus_label_pos(p, c, &branch_visuals),
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

fn midpoint(points: &[Pos2]) -> Pos2 {
    let sum = points.iter().fold(Vec2::ZERO, |acc, p| acc + p.to_vec2());
    (sum / points.len() as f32).to_pos2()
}

/// Direção estável do rótulo do ramo, escolhida em coordenadas-mundo.
fn edge_label_direction(points: &[Pos2]) -> Vec2 {
    let center = midpoint(points);
    let dirs = [
        Vec2::new(0.0, -1.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(-1.0, 0.0),
        Vec2::new(1.0, -1.0).normalized(),
        Vec2::new(-1.0, -1.0).normalized(),
    ];

    dirs.into_iter()
        .enumerate()
        .map(|(i, dir)| {
            let pos = center + dir * LABEL_DIRECTION_SCORE_OFFSET;
            let score = distance_to_branch(pos, points, center) - i as f32 * 0.01;
            (dir, score)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(dir, _)| dir)
        .unwrap_or(Vec2::new(0.0, -1.0))
}

/// Posição do rótulo do ramo fora da linha, usando uma direção estável.
fn edge_label_pos(points: &[Pos2], dir: Vec2) -> Pos2 {
    let offset = if points.len() > 2 {
        MULTITERMINAL_EDGE_LABEL_OFFSET
    } else {
        EDGE_LABEL_OFFSET
    };

    midpoint(points) + dir * offset
}

/// Posição do rótulo do barramento com menor chance de sobrepor ramos e seus rótulos.
fn bus_label_pos(world_node: Pos2, screen_node: Pos2, branches: &[BranchVisual]) -> Pos2 {
    let offset = NODE_R + BUS_LABEL_OFFSET;
    let dirs = [
        Vec2::new(0.0, -1.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(-1.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, -1.0).normalized(),
        Vec2::new(-1.0, -1.0).normalized(),
        Vec2::new(1.0, 1.0).normalized(),
        Vec2::new(-1.0, 1.0).normalized(),
    ];

    dirs.into_iter()
        .enumerate()
        .map(|(i, dir)| {
            let pos = world_node + dir * LABEL_DIRECTION_SCORE_OFFSET;
            let score = bus_label_score(pos, branches) - i as f32 * 0.01;
            (dir, score)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(dir, _)| screen_node + dir * offset)
        .unwrap_or(screen_node + Vec2::new(0.0, -offset))
}

fn bus_label_score(pos: Pos2, branches: &[BranchVisual]) -> f32 {
    let segment_distance = branches
        .iter()
        .flat_map(|branch| branch.world_segments.iter())
        .map(|&(a, z)| dist_to_segment(pos, a, z))
        .fold(f32::INFINITY, f32::min)
        .min(48.0);

    let label_distance = branches
        .iter()
        .filter_map(|branch| branch.world_label_pos)
        .map(|label_pos| pos.distance(label_pos))
        .fold(f32::INFINITY, f32::min)
        .min(48.0);

    segment_distance + label_distance
}

fn distance_to_branch(p: Pos2, points: &[Pos2], center: Pos2) -> f32 {
    if points.len() == 2 {
        return dist_to_segment(p, points[0], points[1]);
    }

    points
        .iter()
        .map(|&endpoint| dist_to_segment(p, center, endpoint))
        .fold(f32::INFINITY, f32::min)
}

fn paint_segment(
    painter: &egui::Painter,
    a: Pos2,
    z: Pos2,
    largura: f32,
    cor: Color32,
    tracejada: bool,
) {
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
            let points: Vec<Pos2> = b
                .nodes
                .iter()
                .filter_map(|node| positions.get(node).copied())
                .map(|w| to_screen(center, pan, zoom, w))
                .collect();
            if points.len() < 2 {
                return None;
            }
            let d = if points.len() == 2 {
                dist_to_segment(p, points[0], points[1])
            } else {
                let m = midpoint(&points);
                points
                    .iter()
                    .map(|&point| dist_to_segment(p, m, point))
                    .fold(f32::INFINITY, f32::min)
            };
            Some((i, d))
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
        // Linha: cinza-claro; mais grossa se carrega consumidores.
        Element::Line { consumers } => {
            let l = if *consumers > 0 { 3.0 } else { 1.5 };
            (Color32::from_gray(170), l, false)
        }
    }
}

/// Rótulo de uma aresta: apenas a quantidade de consumidores do ramo, se houver.
fn edge_label(el: &Element) -> Option<String> {
    match el {
        Element::Line { consumers } if *consumers > 0 => Some(consumers.to_string()),
        Element::Line { .. } => None,
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
