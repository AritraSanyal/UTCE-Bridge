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

        // FIX: -- HARDWARE:  Reverted to the format we know camera accepts
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
            // camera.frame() is blocking. It keeps perfect sync with the camera hardware tick.
            if let Ok(frame) = camera.frame() {
                if let Ok(dynamic_img) = frame.decode_image::<RgbFormat>() {
                    let mut img = DynamicImage::ImageRgb8(dynamic_img);
                    
                    // SOFTWARE FIX: Since the camera won't do it natively, we shrink it 
                    // extremely fast here so the UI thread doesn't choke on a 1080p image.
                    img = img.resize_exact(640, 480, image::imageops::FilterType::Nearest);

                    if tx_frame.send(img).is_err() {
                        break; 
                    }
                }
            }
            // Notice: No thread::sleep() here. We run as fast as the camera allows.
        }
    });

    // 5. MAIN UI LOOP
    let mut current_frame_protocol: Option<StatefulProtocol> = None;

    loop {
        // PERFORMANCE FIX: Drain the channel backlog
        // This ensures the terminal NEVER falls behind the physical camera
        let mut latest_frame = None;
        while let Ok(dynamic_image) = rx_frame.try_recv() {
            latest_frame = Some(dynamic_image);
        }

        if let Some(img) = latest_frame {
            current_frame_protocol = Some(picker.new_resize_protocol(img));
        }

        // Draw the UI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
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

        // 6. HANDLE KEYBOARD EVENTS
        // PERFORMANCE FIX: 1ms poll ensures the UI loop runs at maximum speed
        if event::poll(Duration::from_millis(1))? {
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
