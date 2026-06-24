//! Checksums da rede do exercício de referência contra o gabarito.
//! Trava a topologia conforme a vamos encodando (por ora, só o SD1).

use decfec::fault::{Action, Event, Scenario};
use decfec::topology::{BusKind, Network};

fn net() -> Network {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/networks/ref-exercise.ron");
    let text = std::fs::read_to_string(path).expect("ler networks/ref-exercise.ron");
    let net = Network::from_ron(&text).expect("RON válido");
    net.validate().expect("rede válida");
    net
}

fn ev(at_min: f64, branch: &str, action: Action) -> Event {
    Event {
        at_min,
        branch: branch.into(),
        action,
    }
}

#[test]
fn sd1_checksums_match_gabarito() {
    let net = net();
    let down = |s: &str| net.consumers_of(&net.downstream_lines(s).unwrap());

    assert_eq!(down("1"), 5400, "Cc(SD1) = jusante da cabeceira");
    assert_eq!(down("3"), 4500, "afetado por F2 = jusante da chave 3");
    assert_eq!(down("6"), 1700, "jusante da chave 6 (esquerda 7-8-9)");
}

/// Item (a): DEC e FEC da chave 1 (alimentador SD1) com as faltas F1 e F2.
/// As pontas das NAs usadas viram fontes temporárias (stand-ins de SD2/SD3).
#[test]
fn item_a_dec_fec_match_gabarito() {
    let mut net = net();
    for b in &mut net.buses {
        if b.id == "na6_far" || b.id == "na2_far" {
            b.kind = BusKind::Substation;
        }
    }

    use Action::*;
    let scenario = Scenario {
        events: vec![
            // ---- F2 no trecho 3-4 (reparo em 276 min) ----
            ev(0.0, "tr_3_4", Fault),
            ev(0.0, "1", Open), // disjuntor derruba o alimentador
            // isola a falta, isola 6 e 10, religa o montante (bloco 900)
            ev(2.0, "3", Open),
            ev(2.0, "4", Open),
            ev(2.0, "6", Open),
            ev(2.0, "10", Open),
            ev(2.0, "1", Close),
            ev(40.0, "NA6", Close), // jusante de 6 (1700) -> SD2
            ev(65.0, "NA2", Close), // ramo da chave 5 + tap (1700) -> SD3
            // reparo e volta ao normal (10-11 = 1100 só volta aqui)
            ev(276.0, "tr_3_4", Repair),
            ev(276.0, "3", Close),
            ev(276.0, "4", Close),
            ev(276.0, "6", Close),
            ev(276.0, "10", Close),
            ev(276.0, "NA6", Open),
            ev(276.0, "NA2", Open),
            // ---- F1 no trecho 6-7 (160 min, sem transferência) ----
            ev(300.0, "tr_6_7", Fault),
            ev(300.0, "1", Open),
            ev(302.0, "6", Open), // isola a esquerda; religa o resto
            ev(302.0, "1", Close),
            ev(460.0, "tr_6_7", Repair),
            ev(460.0, "6", Close),
        ],
    };

    let res = scenario.simulate(&net).unwrap();
    let ind = res.indicators(&net, &net.downstream_lines("1").unwrap());

    assert!(
        (ind.dec_h - 2.33).abs() < 0.01,
        "DEC(1) = {} h (esperado ~2,33)",
        ind.dec_h
    );
    assert!(
        (ind.fec - 1.15).abs() < 0.01,
        "FEC(1) = {} (esperado ~1,15)",
        ind.fec
    );
}
