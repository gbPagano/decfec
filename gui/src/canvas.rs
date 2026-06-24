//! Desenho do grafo da rede num canvas egui (somente leitura nesta etapa).
//!
//! As **coordenadas dos nós vivem só aqui/na UI** (um `HashMap<id, Pos2>` em
//! coordenadas-mundo) — o domínio `decfec` não tem geometria. A cada quadro o
//! grafo é ajustado (fit-to-view) ao retângulo disponível, preservando o
//! aspecto, então o tamanho da janela não afeta as posições lógicas.

use std::collections::{BTreeMap, HashMap, VecDeque};

use egui::{Color32, FontId, Pos2, Rect, Stroke, Vec2, emath::RectTransform};

use decfec::topology::{Element, Network, State};

/// Espaçamento entre camadas (eixo X) e entre nós de uma camada (eixo Y), em
/// coordenadas-mundo.
const DX: f32 = 180.0;
const DY: f32 = 80.0;
/// Raio do nó, em pixels de tela.
const NODE_R: f32 = 7.0;

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

/// Desenha o grafo no `ui`. Apenas leitura: arestas (coloridas por tipo, com
/// rótulo) e nós (subestações destacadas).
pub fn draw(ui: &mut egui::Ui, net: &Network, positions: &HashMap<String, Pos2>) {
    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::hover());
    let screen = response.rect;

    let Some(to_screen) = fit_transform(positions, screen) else {
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            "rede vazia",
            FontId::proportional(14.0),
            ui.visuals().weak_text_color(),
        );
        return;
    };

    // Arestas primeiro, para os nós ficarem por cima.
    for b in &net.branches {
        let (Some(&p_from), Some(&p_to)) = (positions.get(&b.from), positions.get(&b.to)) else {
            continue;
        };
        let a = to_screen * p_from;
        let z = to_screen * p_to;
        let (cor, largura, tracejada) = edge_style(&b.element);

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

        // Rótulo no ponto médio.
        painter.text(
            a.lerp(z, 0.5),
            egui::Align2::CENTER_CENTER,
            edge_label(b),
            FontId::proportional(11.0),
            cor,
        );
    }

    // Nós.
    for bus in &net.buses {
        let Some(&p) = positions.get(&bus.id) else {
            continue;
        };
        let c = to_screen * p;
        let (preenchimento, contorno) = if bus.is_source() {
            (Color32::from_rgb(80, 130, 230), Color32::WHITE)
        } else {
            (Color32::from_gray(70), Color32::from_gray(180))
        };
        painter.circle(c, NODE_R, preenchimento, Stroke::new(1.5, contorno));
        painter.text(
            c + Vec2::new(0.0, -NODE_R - 8.0),
            egui::Align2::CENTER_CENTER,
            &bus.id,
            FontId::proportional(11.0),
            ui.visuals().text_color(),
        );
    }
}

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
fn edge_label(b: &decfec::topology::Branch) -> String {
    match b.element {
        Element::Line { consumers } if consumers > 0 => {
            format!("{} ({}c)", b.label(), consumers)
        }
        _ => b.label(),
    }
}

/// Monta a transformação mundo→tela que encaixa todos os nós no `screen`,
/// preservando o aspecto e deixando uma margem. `None` se não há nós.
fn fit_transform(positions: &HashMap<String, Pos2>, screen: Rect) -> Option<RectTransform> {
    let mut it = positions.values();
    let first = *it.next()?;
    let mut mundo = Rect::from_min_max(first, first);
    for &p in it {
        mundo.extend_with(p);
    }
    // Evita divisão por zero em redes degeneradas (1 nó / colineares).
    let mundo = mundo.expand(1.0);

    let avail = screen.shrink(40.0);
    let s = (avail.width() / mundo.width())
        .min(avail.height() / mundo.height())
        .max(f32::MIN_POSITIVE);
    let destino = Rect::from_center_size(
        avail.center(),
        Vec2::new(mundo.width() * s, mundo.height() * s),
    );
    Some(RectTransform::from_to(mundo, destino))
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
