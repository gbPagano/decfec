//! `decfec` — modelo e cálculo de DEC/FEC de redes de distribuição.
//!
//! A biblioteca concentra o domínio (topologia da rede e, futuramente, motor de
//! faltas e métricas), de modo que tanto o binário CLI quanto uma eventual
//! interface gráfica possam reutilizá-la.

pub mod fault;
pub mod topology;
