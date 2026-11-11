//! Clé distante, par le protocole `rtl_tcp`.
//!
//! Permet d'écouter une clé branchée sur une autre machine — un Raspberry Pi
//! près de l'antenne, par exemple — ou de partager la clé du conteneur LXC.
//!
//! Le protocole est simple : le serveur envoie un en-tête de 12 octets, puis le
//! flux I/Q brut ; le client envoie des commandes de 5 octets, un code suivi
//! d'un entier en gros-boutien.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::{Gain, Source};

/// Codes de commande du protocole `rtl_tcp`.
mod command {
    pub const SET_FREQUENCY: u8 = 0x01;
    pub const SET_SAMPLE_RATE: u8 = 0x02;
    pub const SET_GAIN_MODE: u8 = 0x03;
    pub const SET_GAIN: u8 = 0x04;
    pub const SET_FREQ_CORRECTION: u8 = 0x05;
    pub const SET_AGC_MODE: u8 = 0x08;
    pub const SET_BIAS_TEE: u8 = 0x0e;
}

pub struct RtlTcp {
    stream: TcpStream,
    address: SocketAddr,
    tuner: &'static str,
    sample_rate: u32,
    center_freq: u32,
}

impl RtlTcp {
    /// Se connecte à un serveur `rtl_tcp`.
    pub fn connect(address: &str) -> Result<Self> {
        let resolved: SocketAddr = address
            .to_socket_addrs()
            .with_context(|| format!("adresse illisible : {address}"))?
            .next()
            .with_context(|| format!("aucune adresse pour {address}"))?;

        let mut stream = TcpStream::connect_timeout(&resolved, Duration::from_secs(10))
            .with_context(|| format!("connexion à {resolved} impossible"))?;
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok();

        // En-tête : « RTL0 », type de tuner, nombre de paliers de gain.
        let mut header = [0u8; 12];
        stream
            .read_exact(&mut header)
            .context("en-tête rtl_tcp absent : le serveur est-il bien rtl_tcp ?")?;
        if &header[0..4] != b"RTL0" {
            bail!("ce n'est pas un serveur rtl_tcp (en-tête « {:?} »)", &header[0..4]);
        }
        let tuner = tuner_name(u32::from_be_bytes([
            header[4], header[5], header[6], header[7],
        ]));

        let mut client = Self {
            stream,
            address: resolved,
            tuner,
            sample_rate: 0,
            center_freq: 0,
        };
        client.send(command::SET_AGC_MODE, 0)?;
        client.set_sample_rate(crate::dsp::DEFAULT_SAMPLE_RATE)?;
        Ok(client)
    }

    fn send(&mut self, command: u8, value: u32) -> Result<()> {
        let mut message = [0u8; 5];
        message[0] = command;
        message[1..].copy_from_slice(&value.to_be_bytes());
        self.stream
            .write_all(&message)
            .with_context(|| format!("commande {command:#04x} refusée par {}", self.address))
    }
}

fn tuner_name(code: u32) -> &'static str {
    match code {
        1 => "E4000",
        2 => "FC0012",
        3 => "FC0013",
        4 => "FC2580",
        5 => "R820T",
        6 => "R828D",
        _ => "inconnu",
    }
}

impl Source for RtlTcp {
    fn name(&self) -> String {
        format!("rtl_tcp {} (tuner {})", self.address, self.tuner)
    }

    fn set_center_freq(&mut self, hz: u32) -> Result<()> {
        self.send(command::SET_FREQUENCY, hz)?;
        self.center_freq = hz;
        Ok(())
    }

    fn center_freq(&self) -> u32 {
        self.center_freq
    }

    fn set_sample_rate(&mut self, hz: u32) -> Result<()> {
        self.send(command::SET_SAMPLE_RATE, hz)?;
        self.sample_rate = hz;
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn set_gain(&mut self, gain: Gain) -> Result<()> {
        match gain {
            Gain::Auto => self.send(command::SET_GAIN_MODE, 0),
            Gain::Manual(tenths) => {
                self.send(command::SET_GAIN_MODE, 1)?;
                self.send(command::SET_GAIN, tenths as u32)
            }
        }
    }

    fn set_ppm(&mut self, ppm: i32) -> Result<()> {
        self.send(command::SET_FREQ_CORRECTION, ppm as u32)
    }

    fn set_bias_tee(&mut self, on: bool) -> Result<()> {
        self.send(command::SET_BIAS_TEE, u32::from(on))
    }

    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        let read = self
            .stream
            .read(out)
            .with_context(|| format!("flux interrompu depuis {}", self.address))?;
        if read == 0 {
            bail!("le serveur {} a fermé la connexion", self.address);
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn les_tuners_connus_sont_nommes() {
        assert_eq!(tuner_name(5), "R820T");
        assert_eq!(tuner_name(6), "R828D"); // celui de la clé V4
        assert_eq!(tuner_name(42), "inconnu");
    }

    #[test]
    fn un_serveur_sans_entete_est_refuse() {
        // Un serveur qui n'est pas rtl_tcp doit donner un message clair plutôt
        // qu'un flux d'octets interprété de travers.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n");
            }
        });

        let err = match RtlTcp::connect(&address.to_string()) {
            Ok(_) => panic!("un serveur sans en-tête rtl_tcp doit être refusé"),
            Err(err) => err,
        };
        assert!(
            format!("{err:#}").contains("rtl_tcp"),
            "message peu clair : {err:#}"
        );
    }

    #[test]
    fn l_entete_annonce_le_tuner() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut header = [0u8; 12];
                header[0..4].copy_from_slice(b"RTL0");
                header[4..8].copy_from_slice(&6u32.to_be_bytes()); // R828D
                header[8..12].copy_from_slice(&29u32.to_be_bytes());
                let _ = socket.write_all(&header);
                // Le client envoie ses réglages ; on garde la connexion ouverte.
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let client = RtlTcp::connect(&address.to_string()).unwrap();
        assert!(client.name().contains("R828D"), "{}", client.name());
    }
}
