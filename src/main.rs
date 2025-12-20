use std::fs;

use libmtp_rs::device::raw::detect_raw_devices;

const PLAYLISTS_FOLDER: &str = "playlists";
const GARMIN_VENDOR_ID: u16 = 2334;

fn main() {

    let devices = detect_raw_devices();
    if devices.is_err() {
        eprintln!("No device attached");
        return;
    }
    let device = match devices.unwrap().iter().find(|d| d.device_entry().vendor_id == GARMIN_VENDOR_ID) {
        Some(d) => d,
        None => {
            eprintln!("No Garmin device found");
            return;
        }
    };

    create_data_folder_idempotent();

    if let Ok(playlists) = fs::read_dir(PLAYLISTS_FOLDER) {
        for playlist in playlists.flatten() {
            println!("Found Playlist {}", playlist.file_name().to_string_lossy());
            
        }
    };
}

fn create_data_folder_idempotent() {
    if fs::create_dir(PLAYLISTS_FOLDER).is_err() {
        println!("playlists directory already exists, continuing");
    }
}
