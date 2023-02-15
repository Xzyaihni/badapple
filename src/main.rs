use std::{
    env,
    thread,
    process,
    time::{Duration, Instant}
};

use ffmpeg_next::{
    codec,
    format::{self, Pixel},
    media::Type,
    software::scaling::{self, flag::Flags},
    util::frame::video::Video
};


fn show_frame(pixels: &[u8], real_width: usize)
{
    let real_height = pixels.len() / real_width;

    let negative_pad_x = real_width % 2;
    let negative_pad_y = (real_height % 4).min(1);

    let width = real_width / 2 - negative_pad_x;
    let height = real_height / 4 - negative_pad_y;

    print!("\x1b[0;0H");
    for y in 0..height
    {
        for x in 0..width
        {
            let pixel = |index: usize|
            {
                let index_x = index % 2;
                let index_y = index / 2;

                pixels[(y * 4 + index_y) * real_width + (x * 2 + index_x)]
            };

            let chunk = (0..8).map(|index|
            {
                pixel(index) < 127
            }).collect::<Vec<bool>>().try_into().unwrap();

            print!("{}", get_braille(chunk));
        }

        println!();
    }
}

fn get_braille(pixels: [bool; 8]) -> char
{
    let index: u32 = pixels.iter().enumerate().map(|(index, point)|
    {
        let index = if index % 2 == 0
        {
            index / 2
        } else
        {
            index + (8 - index) / 2
        };

        let index = if index == 3 {
            6
        } else if index > 3 && index < 7
        {
            index - 1
        } else
        {
            index
        };

        if *point
        {
            1 << index
        } else {0}
    }).sum();

    char::from_u32(0x2800 + index).unwrap()
}

fn main()
{
    let video_path = env::args().nth(1).unwrap_or_else(||
    {
        eprintln!("usage: {} path/to/badapple.mp4", env::args().next().unwrap());
        process::exit(1)
    });

    ffmpeg_next::init().unwrap();
    let mut input_context = format::input(&video_path).unwrap();
    let input = input_context.streams().best(Type::Video).unwrap();

    let video_stream_index = input.index();

    let context_decoder = codec::context::Context::from_parameters(input.parameters()).unwrap();
    let mut decoder = context_decoder.decoder().video().unwrap();

    let size = terminal_size::terminal_size().unwrap();

    let width = size.0.0 as usize * 2;
    let height = (size.1.0 as usize * 4) - 1;

    let mut scaling_context = scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        Pixel::GRAY8,
        width as u32,
        height as u32,
        Flags::BILINEAR
    ).unwrap();

    let mut previous_frame = Instant::now();
    for (stream, packet) in input_context.packets()
    {
        if stream.index() == video_stream_index
        {
            decoder.send_packet(&packet).unwrap();

            let mut decoded = Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok()
            {
                let mut frame = Video::empty();
                scaling_context.run(&decoded, &mut frame).unwrap();

                show_frame(frame.data(0), width);

                //i dont get the time base stuff ; -; its not the inverse of the actual fps..
                let duration = f64::from(stream.rate().invert()) * 1000.0;
                let frame_duration = Duration::from_millis(duration as u64);

                if let Some(to_next) = frame_duration.checked_sub(previous_frame.elapsed())
                {
                    thread::sleep(to_next)
                }

                previous_frame = Instant::now();
            }
        }
    }

    decoder.send_eof().unwrap();
}