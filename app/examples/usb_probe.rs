//! Repérage de la clé RTL-SDR : cargo run --example usb_probe
fn main() {
use nusb::MaybeFuture;
    for d in nusb::list_devices().wait().unwrap() {
        if d.vendor_id() == 0x0bda {
            println!(
                "{:04x}:{:04x} bus {} addr {} — {:?} / {:?} / {:?}",
                d.vendor_id(),
                d.product_id(),
                d.bus_id(),
                d.device_address(),
                d.manufacturer_string(),
                d.product_string(),
                d.serial_number()
            );
            match d.open().wait() {
                Ok(dev) => {
                    println!("  ouverture : ok");
                    match dev.claim_interface(0).wait() {
                        Ok(_) => println!("  interface 0 : réservée"),
                        Err(e) => println!("  interface 0 : {e}"),
                    }
                }
                Err(e) => println!("  ouverture : {e}"),
            }
        }
    }
}
