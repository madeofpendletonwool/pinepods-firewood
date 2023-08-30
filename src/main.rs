use std::fs::{File, read_dir};
// use std::time::Duration;
use rodio::{Decoder, OutputStream, source::Source};
use mp3_duration;
use std::collections::HashMap;

// fn print_type_of<T>(_: &T) {
//     println!("{}", std::any::type_name::<T>())
// }
fn play_audio() {
    println!("Here are the songs available to play:");
    let path = read_dir("music").unwrap();
    for entry in path {
        let track_name = entry.unwrap().file_name().into_string().unwrap();
        println!("{}", &track_name[0..track_name.find("music/").unwrap_or(track_name.len())]);
    };
    println!("");
    println!("Please type a track name to play");

    let mut user_song: String = String::new();
    std::io::stdin().read_line(&mut user_song).unwrap();
    let file_path = format!("music/{}", user_song.trim());

    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let file = File::open(&file_path).unwrap();
    let source = Decoder::new(file).unwrap();

    let duration = mp3_duration::from_path(&file_path).unwrap();
    println!("Duration: {:?}", duration);

    match stream_handle.play_raw(source.convert_samples()) {
        Ok(_) => {}
        Err(e) => println!("An error occurred: {}", e),
    };

    std::thread::sleep(duration);
}

fn make_request(hostname: &str) {
    let result = reqwest::get(hostname);

    print!("test");
    println!("{:?}", result);
}

fn main() {
    println!("Hello! Welcome to Pinepods Firewood!");
    println!("We'll first need to connect you to your Pinepods Server. Please enter your hostname below:");

    let mut hostname: String = String::new();
    std::io::stdin().read_line(&mut hostname).unwrap();

    let return_value = make_request(hostname.as_str());
    

}
