use std::{collections::HashMap, fs::{self, DirEntry}, io::{self}, os::unix::fs::MetadataExt, path::Path};

use audiotags::Tag;
use libmtp_rs::{chrono::DateTime, device::raw::detect_raw_devices, object::{filetypes::Filetype, Object}, storage::{files::FileMetadata, Parent}, util::CallbackReturn};

const PLAYLISTS_FOLDER: &str = "playlists";
const TMP_FOLDER: &str = "tmp";
const GARMIN_VENDOR_ID: u16 = 2334;

fn main() {

    let devices = detect_raw_devices();
    if devices.is_err() {
        eprintln!("No device attached");
        return;
    }
    let device = match devices.unwrap().into_iter().find(|d| d.device_entry().vendor_id == GARMIN_VENDOR_ID) {
        Some(d) => d,
        None => {
            eprintln!("No Garmin device found");
            return;
        }
    };
    println!("Device found: {:?}", &device);

    if let Ok(_) = fs::read_dir(TMP_FOLDER) {
        print!("Found still existing tmp folder, removing...");
        delete_tmp();
        println!("DONE");
    }

    if fs::create_dir_all(PLAYLISTS_FOLDER).is_ok() {
        println!("Created playlist directory");
    }

    println!("Reading playlists...");

    let mut upload_tracks: HashMap<String, HashMap<String, Vec<DirEntry>>> = HashMap::new();
    let mut upload_playlists: Vec<(String, Vec<String>)> = Vec::new();

    let playlists = match fs::read_dir(PLAYLISTS_FOLDER) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error reading playlists folder: {}", e);
            return;
        }
    };

    for playlist in playlists.flatten() {
        if playlist.file_type().is_err() || !playlist.file_type().unwrap().is_dir() {
            println!("Skipping a non directory file...");
            continue;
        }
        let playlist_name = playlist.file_name().to_string_lossy().to_string();
        println!("Found Playlist {}", playlist_name);

        let mut upload_playlist = Vec::new();
        
        for track in fs::read_dir(playlist.path()).unwrap().flatten() {
            
            let tag = Tag::new().read_from_path(track.path()).unwrap();
            let file_name = match track.file_name().into_string() {
                Ok(s) => s,
                Err(_) => String::from("Unknown.mp3")
            };
            if !file_name.to_lowercase().ends_with("mp3") {
                println!("{} is not an mp3, skipping", file_name);
                continue;
            }
            println!("  Found {}", file_name);
            let artist = adjust_file_name(tag.artist().unwrap_or("Unknown").to_owned());
            let album = adjust_file_name(tag.album_title().unwrap_or("Unknown").to_owned());

            upload_tracks
            .entry(artist.clone()).or_default()
            .entry(album.clone()).or_default().push(track);

            let track_path_on_watch = format!("0:/music/{}/{}/{}", artist, album, file_name).to_uppercase();
            upload_playlist.push(track_path_on_watch);
        }

        upload_playlists.push((playlist_name, upload_playlist));
    }

    println!("Reading playlists done");

    print!("Copying files...");
    
    if let Err(e) = copy_dir_all(PLAYLISTS_FOLDER, TMP_FOLDER) {
        eprintln!("Error copying files: {}", e);
        return;
    }
    println!("DONE");

    print!("Adjusting metadata...");

    if let Err(e) = adjust_all_metadata(TMP_FOLDER) {
        eprintln!("Error adjusting metadata: {}", e);
        return;
    }

    println!("DONE");

    let device = match device.open_uncached() {
        Some(d) => d,
        None => {
            eprintln!("Could not open device, is it already in use?");
            return;
        }
    };

    let storages: Vec<u32> = device.storage_pool().iter().map(|(id, _)| id).collect();

    if storages.len() > 1 {
        println!("Found more than one storage, using the first one");
    }
    let pool = device.storage_pool();
    let storage = pool.by_id(*storages.first().unwrap()).unwrap();

    let music_folder = match storage.files_and_folders(Parent::Root).into_iter().find(|f| f.name() == "Music") {
        Some(f) => Parent::Folder(f.id()),
        None => {
            eprintln!("Could not find Music folder");
            return;
        }
    };

    println!("Clearing storage...");

    for artist in storage.files_and_folders(music_folder).into_iter() {
        if matches!(artist.ftype(), Filetype::Folder) {
            for album in storage.files_and_folders(Parent::Folder(artist.id())) {
                for track in storage.files_and_folders(Parent::Folder(album.id())) {
                    print!("Removing track {}...", track.name());
                    match track.delete() {
                        Ok(_) => println!("OK"),
                        Err(e) => println!("ERROR, {:?}", e),
                    }
                }
                print!("Removing album folder {}...", album.name());
                match album.delete() {
                    Ok(_) => println!("OK"),
                    Err(e) => println!("ERROR, {:?}", e),
                }
            }
        }
        print!("Removing artist folder {}...", artist.name());
        match artist.delete() {
            Ok(_) => println!("OK"),
            Err(e) => println!("ERROR, {:?}", e),
        }
    }

    println!("Clearing done");

    println!("Writing back...");

    for (artist, albums) in upload_tracks {
        print!("Writing artist folder {}...", &artist);
        let artist_folder = match storage.create_folder(&artist, music_folder) {
            Ok((folder, _)) => {
                println!("OK");
                Parent::Folder(folder)
            },
            Err(e) => {
                println!("ERROR writing artist folder, {}", e);
                continue;
            }
        };
        for (album, tracks) in albums {
            print!("Writing album folder {}...", &album);
            let album_folder = match storage.create_folder(&album, artist_folder) {
                Ok((folder, _)) => {
                    println!("OK");
                    Parent::Folder(folder)
                },
                Err(e) => {
                    println!("ERROR writing album folder, {}", e);
                    continue;
                }
            };
            for file in tracks {
                let file_name = file.file_name();
                let metadata = FileMetadata {
                    file_name: file_name.to_str().unwrap(),
                    file_size: file.metadata().unwrap().size(),
                    file_type: Filetype::Mp3,
                    modification_date: DateTime::from(file.metadata().unwrap().modified().unwrap()),
                };

                print!("Writing track {}...", metadata.file_name);
                if let Err(_) = storage.send_file_from_path_with_callback(file.path(), album_folder, metadata, |_, _| CallbackReturn::Continue) {
                    println!("ERROR writing track");
                    continue;
                }
                println!("OK");
            }
        }
    }

    let tmp_playlists_path = TMP_FOLDER.to_owned() + "/playlists";
    if let Err(e) = fs::create_dir(&tmp_playlists_path) {
        eprintln!("Could not create tmp playlist folder: {}", e);
        return;
    }

    for (name, entries) in upload_playlists {
        let mut content = String::new();
        for entry in entries {
            content.push_str(&entry);
            content.push('\n');
        }
        if let Err(e) = fs::write(tmp_playlists_path.clone() + "/" + &name + ".m3u8", content) {
            eprintln!("Could not create playlist file: {}", e);
            return;
        }
    }

    for file in fs::read_dir(&tmp_playlists_path).unwrap().flatten() {

        let file_name = file.file_name();
        let metadata = FileMetadata {
            file_name: file_name.to_str().unwrap(),
            file_size: file.metadata().unwrap().size(),
            file_type: Filetype::Playlist,
            modification_date: DateTime::from(file.metadata().unwrap().modified().unwrap()),
        };

        print!("Writing playlist {}...", metadata.file_name);
        if let Err(_) = storage.send_file_from_path_with_callback(file.path(), music_folder, metadata, |_, _| CallbackReturn::Continue) {
            println!("ERROR writing track");
            continue;
        }
        println!("OK");
    }

    println!("Writing done");

    print!("Cleaning up...");

    delete_tmp();

    println!("DONE");
}

fn adjust_file_name(name: String) -> String {
    return name.replace(|c: char| {
        let alphanumeric = c.is_ascii_alphanumeric();
        let exclude = [' ', '.', '(', ')'];
        !(alphanumeric || exclude.contains(&c))
    }, "_");
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn adjust_all_metadata(src: impl AsRef<Path>) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            adjust_all_metadata(entry.path())?;
        } else {
            let mut tag = Tag::new().read_from_path(entry.path()).unwrap();
            tag.remove_album_cover();
            tag.remove_disc();
            tag.remove_disc_number();
            tag.remove_total_discs();
            tag.remove_total_tracks();
            tag.remove_track();
            tag.remove_track_number();
            if let Err(e) = tag.write_to_path(entry.path().to_str().unwrap()) {
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }

            let title = match tag.title() {
                Some(t) => format!("{}.mp3", t).to_string(),
                None => continue
            };
            let os_file_name = entry.file_name().into_string().unwrap();
            let file_name = os_file_name.as_str();
            let new_path = entry.path().to_string_lossy().replace(file_name, title.as_str());
            if let Err(e) = fs::rename(entry.path(), new_path) {
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }
        }
    }
    Ok(())
}

fn delete_tmp() {
    if let Err(e) = fs::remove_dir_all(TMP_FOLDER) {
        eprintln!("Could not delete tmp folder: {}", e);
        return;
    }
}