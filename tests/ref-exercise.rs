//! Checksums da rede do exercício de referência contra o gabarito.
//! Trava a topologia conforme a vamos encodando (por ora, só o SD1).

use decfec::fault::Scenario;
use decfec::topology::Network;

fn net() -> Network {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/networks/ref-exercise.ron");
    let text = std::fs::read_to_string(path).expect("ler networks/ref-exercise.ron");
    let net = Network::from_ron(&text).expect("RON válido");
    net.validate().expect("rede válida");
    net
}

#[test]
fn sd1_checksums_match_gabarito() {
    let net = net();
    let down = |s: &str| net.consumers_of(&net.downstream_lines(s).unwrap());

    assert_eq!(down("2"), 5400, "Cc(SD1) = jusante da cabeceira");
    assert_eq!(down("3"), 4500, "afetado por F2 = jusante da chave 3");
    assert_eq!(down("6"), 1700, "jusante da chave 6 (esquerda 7-8-9)");
}

/// Item (a): DEC e FEC da chave 2 (alimentador SD1) com as faltas F1 e F2.
#[test]
fn item_a_dec_fec_match_gabarito() {
    let net = net();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/item_a.ron");
    let text = std::fs::read_to_string(path).expect("ler scenarios/item_a.ron");
    let scenario = Scenario::from_ron(&text).expect("RON válido");

    let res = scenario.simulate(&net).unwrap();
    let ind = res.indicators(&net, &net.downstream_lines("2").unwrap());

    assert!((ind.dec_h - 2.33).abs() < 0.01, "DEC = {}", ind.dec_h);
    assert!((ind.fec - 1.15).abs() < 0.01, "FEC = {}", ind.fec);
}
