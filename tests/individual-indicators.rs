use decfec::fault::Scenario;
use decfec::topology::Network;

const NETWORK: &str = r#"
Network(
    buses: [
        (id: "SD1", kind: Substation),
        (id: "A", kind: Switch(normal: Closed)),
        (id: "F2", kind: Junction),
        (id: "B", kind: Switch(normal: Closed)),
        (id: "D", kind: Switch(normal: Closed)),
        (id: "C", kind: Switch(normal: Closed)),
        (id: "E", kind: Switch(normal: Closed)),
        (id: "F1", kind: Junction),
        (id: "X", kind: Junction),
        (id: "NA1", kind: Switch(normal: Open)),
        (id: "G", kind: Switch(normal: Closed)),
        (id: "F", kind: Switch(normal: Closed)),
        (id: "H", kind: Switch(normal: Closed)),
        (id: "SD2", kind: Substation),
        (id: "J", kind: Switch(normal: Closed)),
        (id: "I", kind: Switch(normal: Closed)),
        (id: "F4", kind: Junction),
        (id: "K", kind: Switch(normal: Closed)),
        (id: "NA2", kind: Switch(normal: Open)),
        (id: "b1_copy", kind: Junction),
        (id: "O", kind: Switch(normal: Closed)),
        (id: "N", kind: Switch(normal: Closed)),
        (id: "F3", kind: Junction),
        (id: "P", kind: Switch(normal: Closed)),
        (id: "M", kind: Switch(normal: Closed)),
        (id: "b2", kind: Junction),
        (id: "L", kind: Switch(normal: Closed)),
        (id: "F5", kind: Junction),
        (id: "SD3", kind: Substation),
        (id: "Y", kind: Junction),
    ],
    branches: [
        (id: Some("ramo_SD1_A"), nodes: ["SD1", "A"], element: Line(consumers: 0)),
        (id: Some("ramo_A_F2"), nodes: ["A", "F2"], element: Line(consumers: 0)),
        (id: Some("ramo_A_B"), nodes: ["F2", "B"], element: Line(consumers: 300)),
        (id: Some("ramo_D_C_B"), nodes: ["D", "C", "B"], element: Line(consumers: 200)),
        (id: Some("ramo_D_E"), nodes: ["D", "E"], element: Line(consumers: 100)),
        (id: Some("ramo_F1_E"), nodes: ["F1", "E"], element: Line(consumers: 0)),
        (id: Some("ramo_F1_b1"), nodes: ["F1", "X"], element: Line(consumers: 50)),
        (id: Some("ramo_NA1_C"), nodes: ["NA1", "C"], element: Line(consumers: 150)),
        (id: Some("ramo_NA1_G"), nodes: ["NA1", "G"], element: Line(consumers: 100)),
        (id: Some("ramo_H_G_F"), nodes: ["H", "G", "F"], element: Line(consumers: 300)),
        (id: Some("ramo_SD2_F"), nodes: ["SD2", "F"], element: Line(consumers: 0)),
        (id: Some("ramo_J_H_I"), nodes: ["J", "H", "I"], element: Line(consumers: 400)),
        (id: Some("ramo_J_F4"), nodes: ["J", "F4"], element: Line(consumers: 200)),
        (id: Some("ramo_NA2_K_I"), nodes: ["NA2", "K", "I"], element: Line(consumers: 100)),
        (id: Some("ramo_b1_copy_K"), nodes: ["b1_copy", "K"], element: Line(consumers: 100)),
        (id: Some("ramo_F3_N"), nodes: ["F3", "N"], element: Line(consumers: 0)),
        (id: Some("ramo_O_F3"), nodes: ["O", "F3"], element: Line(consumers: 200)),
        (id: Some("ramo_P_b2"), nodes: ["P", "b2"], element: Line(consumers: 150)),
        (id: Some("ramo_P_M_N"), nodes: ["P", "M", "N"], element: Line(consumers: 300)),
        (id: Some("ramo_L_F5"), nodes: ["L", "F5"], element: Line(consumers: 0)),
        (id: Some("ramo_F5_M"), nodes: ["F5", "M"], element: Line(consumers: 500)),
        (id: Some("ramo_SD3_L"), nodes: ["SD3", "L"], element: Line(consumers: 0)),
        (id: Some("ramo_O_Y_NA2"), nodes: ["O", "Y", "NA2"], element: Line(consumers: 100)),
    ],
)
"#;

const SCENARIO: &str = r#"
Scenario(
    events: [
        (at_min: 0.0, bus: "F1", action: Fault),
        (at_min: 0.0, bus: "E", action: Open),
        (at_min: 120.0, bus: "F1", action: Repair),
        (at_min: 120.0, bus: "E", action: Close),
        (at_min: 200.0, bus: "F2", action: Fault),
        (at_min: 200.0, bus: "A", action: Open),
        (at_min: 220.0, bus: "B", action: Open),
        (at_min: 220.0, bus: "NA1", action: Close),
        (at_min: 250.0, bus: "F2", action: Repair),
        (at_min: 250.0, bus: "A", action: Close),
        (at_min: 250.0, bus: "NA1", action: Open),
        (at_min: 250.0, bus: "B", action: Close),
        (at_min: 300.0, bus: "F3", action: Fault),
        (at_min: 300.0, bus: "N", action: Open),
        (at_min: 480.0, bus: "F3", action: Repair),
        (at_min: 480.0, bus: "N", action: Close),
        (at_min: 500.0, bus: "F4", action: Fault),
        (at_min: 500.0, bus: "J", action: Open),
        (at_min: 540.0, bus: "F4", action: Repair),
        (at_min: 540.0, bus: "J", action: Close),
        (at_min: 600.0, bus: "F5", action: Fault),
        (at_min: 600.0, bus: "L", action: Open),
        (at_min: 690.0, bus: "M", action: Open),
        (at_min: 690.0, bus: "NA2", action: Close),
        (at_min: 840.0, bus: "F5", action: Repair),
        (at_min: 840.0, bus: "L", action: Close),
        (at_min: 840.0, bus: "NA2", action: Open),
        (at_min: 840.0, bus: "M", action: Close),
    ],
)
"#;

#[test]
fn calcula_dic_fic_dmic_para_x_e_y() {
    let net = Network::from_ron(NETWORK).expect("rede válida");
    net.validate().expect("topologia válida");
    let scenario = Scenario::from_ron(SCENARIO).expect("cenário válido");
    let result = scenario.simulate(&net).expect("simulação válida");

    let x = result
        .individual_indicators_for_bus(&net, "X")
        .expect("X é um ponto consumidor");
    assert!((x.dic_h - 2.33).abs() < 0.01, "DIC X = {}", x.dic_h);
    assert_eq!(x.fic, 2, "FIC X");
    assert!((x.dmic_h - 2.0).abs() < 0.01, "DMIC X = {}", x.dmic_h);

    let y = result
        .individual_indicators_for_bus(&net, "Y")
        .expect("Y é um ponto consumidor");
    assert!((y.dic_h - 4.50).abs() < 0.01, "DIC Y = {}", y.dic_h);
    assert_eq!(y.fic, 2, "FIC Y");
    assert!((y.dmic_h - 3.0).abs() < 0.01, "DMIC Y = {}", y.dmic_h);
}
