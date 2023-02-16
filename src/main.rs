use std::{
    thread,
    path::Path,
    time::{Duration, Instant}
};

use argparse::{ArgumentParser, StoreFalse, Store};

use ffmpeg_next::{
    codec,
    format::{self, Pixel},
    media::Type,
    software::scaling::{self, flag::Flags},
    util::frame::video::Video
};


struct Pos
{
    x: i32,
    y: i32
}

fn show_frame(
    pixels: &[u8],
    options: &Options,
    skip_width: usize,
    scaled_width: usize,
    scaled_height: usize
)
{
    let width = scaled_width * 2;
    let height = scaled_height * 4;

    let mut errors = vec![0.0; width * height];

    print!("\x1b[0;0H");
    for y in 0..scaled_height
    {
        for x in 0..scaled_width
        {
            let chunk = (0..8).map(|index|
            {
                let index_x = index % 2;
                let index_y = index / 2;

                let x = x * 2 + index_x;
                let y = y * 4 + index_y;

                let error_index = y * width + x;

                let pixel = pixels[y * skip_width + x] as f64 + errors[error_index];
                let pixel = pixel.round() as u8;
                errors[error_index] = 0.0;

                let filled = pixel < options.threshold;

                const PATTERN: [(f64, Pos); 4] = [
                    (7.0, Pos{x: 1, y: 0}),
                    (3.0, Pos{x: -1, y: 1}),
                    (5.0, Pos{x: 0, y: 1}),
                    (1.0, Pos{x: 1, y: 1})
                ];

                let denominator = 16.0;

                if options.dither
                {
                    let error = if filled
                    {
                        pixel
                    } else
                    {
                        pixel - options.threshold
                    };

                    for (amount, pos) in PATTERN
                    {
                        let width = width as i32;
                        let height = height as i32;

                        let x = x as i32 + pos.x;
                        let y = y as i32 + pos.y;
                        if x >= width || x < 0 || y >= height || y < 0
                        {
                            continue;
                        }

                        let scale = amount / denominator;

                        let index = error_index as i32 + pos.y * width + pos.x;
                        errors[index as usize] += error as f64 * scale;
                    }
                }

                filled
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

pub fn terminal_size() -> (usize, usize)
{
    let winsize = libc::winsize{
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0
    };

    unsafe
    {
        libc::ioctl(0, libc::TIOCGWINSZ, &winsize);
    }

    (winsize.ws_col as usize, winsize.ws_row as usize)
}

struct Options
{
    pub dither: bool,
    pub threshold: u8
}

fn main()
{
    ffmpeg_next::init().unwrap();

    let mut options = Options{dither: true, threshold: 127};

    let mut video_path = String::new();

    {
        let mut parser = ArgumentParser::new();

        parser.refer(&mut options.dither)
            .add_option(&["-d", "--dither"], StoreFalse, "disable dithering");

        parser.refer(&mut options.threshold)
            .add_option(&["-t", "--threshold"], Store,
                "threshold value from 0 to 255 (default 127)");

        parser.refer(&mut video_path)
            .add_option(&["-i", "--input"], Store, "path to the video file")
            .add_argument("video_path", Store, "path to the video file")
            .required();

        parser.parse_args_or_exit();
    }

    if !Path::new(&video_path).exists()
    {
        eprintln!("no file at path: \"{video_path}\"");
        return;
    }

    let options = options;

    let mut input_context = format::input(&video_path).unwrap();
    let input = input_context.streams().best(Type::Video).unwrap();

    let video_stream_index = input.index();

    let context_decoder = codec::context::Context::from_parameters(input.parameters()).unwrap();
    let mut decoder = context_decoder.decoder().video().unwrap();

    let size = terminal_size();
    let unscaled_width = size.0 as usize;
    let unscaled_height = size.1 as usize - 1;

    let width = unscaled_width * 2;
    let height = unscaled_height * 4;

    let width_remainder = width % 32;
    let width_adjust = 32 - width_remainder;
    let width_adjust = if width_adjust == 32 {0} else {width_adjust};

    let width_skip = width + width_adjust;

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

                show_frame(frame.data(0), &options, width_skip, unscaled_width, unscaled_height);

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