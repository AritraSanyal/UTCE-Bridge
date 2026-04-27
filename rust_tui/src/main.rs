use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use image::DynamicImage;
use nokhwa::{
    pixel_format::RgbFormat,
    utils::{CameraIndex, RequestedFormat, RequestedFormatType},
    Camera,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol, StatefulImage};
use std::{io, sync::mpsc, thread, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. SETUP TERMINAL
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 2. SETUP IMAGE PICKER
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

    // 3. SETUP CHANNELS FOR MULTITHREADING
    let (tx_frame, rx_frame) = mpsc::channel::<DynamicImage>();

    // 4. START CAMERA THREAD
    thread::spawn(move || {
        let index = CameraIndex::Index(0);
        let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
        
        let mut camera = match Camera::new(index, requested) {
            Ok(cam) => cam,
            Err(e) => {
                eprintln!("Failed to initialize camera: {}", e);
                return;
            }
        };

        camera.open_stream().unwrap();

        loop {
            if let Ok(frame) = camera.frame() {
                if let Ok(dynamic_img) = frame.decode_image::<RgbFormat>() {
                    let img = DynamicImage::ImageRgb8(dynamic_img);
                    if tx_frame.send(img).is_err() {
                        break; 
                    }
                }
            }
            // Cap at ~60 FPS to prevent CPU burnout
            thread::sleep(Duration::from_millis(16)); 
        }
    });

    // 5. MAIN UI LOOP
    let mut current_frame_protocol: Option<StatefulProtocol> = None;

    loop {
        // Non-blocking check for new camera frames
        if let Ok(dynamic_image) = rx_frame.try_recv() {
            current_frame_protocol = Some(picker.new_resize_protocol(dynamic_image));
        }

        // Draw the UI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                // THE FIX: Array passed directly, no .as_ref()
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(f.area());

            // --- VIDEO BLOCK RENDER ---
            let video_block = Block::default().title(" Camera Feed ").borders(Borders::ALL);
            let video_inner_area = video_block.inner(chunks[0]);
            
            f.render_widget(video_block, chunks[0]);

            if let Some(ref mut protocol) = current_frame_protocol {
                let image_widget = StatefulImage::default();
                f.render_stateful_widget(image_widget, video_inner_area, protocol);
            } else {
                let placeholder = Paragraph::new("Waiting for camera...");
                f.render_widget(placeholder, video_inner_area);
            }

            // --- SIDEBAR RENDER ---
            let sidebar_block = Block::default().title(" UTCE-Net Status ").borders(Borders::ALL);
            let sidebar_text = "Status: Waiting for ML backend...\n\nTarget FPS: 15\nEmotion: --\nBuffer: 0/8";
            let sidebar_widget = Paragraph::new(sidebar_text).block(sidebar_block);
            f.render_widget(sidebar_widget, chunks[1]);
        })?;

        // 6. HANDLE KEYBOARD EVENTS (Exit on 'q')
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    // 7. CLEANUP AND EXIT
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
