mod error_msg;

use crate::error_msg::print_last_error;
use common::ItemInfo;
use native_windows_gui as nwg;
use std::{cell::RefCell, mem::size_of, ptr::null_mut, rc::Rc};
use windows::core::imp::HANDLE;
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileA, ReadFile, OPEN_EXISTING},
};

const DEVICE_PATH: &[u8] = b"\\\\.\\Valhalla\0";
const POLL_INTERVAL_MS: u64 = 500;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--cli") {
        run_cli();
    } else {
        run_gui();
    }
}

// ---------------------------------------------------------------------------
// GUI mode
// ---------------------------------------------------------------------------

fn run_gui() {
    if let Err(e) = nwg::init() {
        eprintln!("Failed to initialize NWG: {e}");
        std::process::exit(1);
    }

    let app = Rc::new(RefCell::new(ValhallaApp::default()));

    if let Err(e) = ValhallaApp::build(&app) {
        eprintln!("Failed to build GUI: {e}");
        std::process::exit(1);
    }

    let app_tick = app.clone();
    nwg::full_bind_event_handler(&app.borrow().window.handle, move |evt, _data, handle| {
        let mut a = app_tick.borrow_mut();
        use nwg::Event;
        match evt {
            Event::OnButtonClick if handle == a.refresh_btn.handle => {
                a.poll_driver();
            },
            Event::OnButtonClick if handle == a.clear_btn.handle => {
                a.clear_events();
            },
            Event::OnTimerTick if handle == a.timer.handle => {
                a.poll_driver();
            },
            Event::OnWindowClose if handle == a.window.handle => {
                nwg::stop_thread_dispatch();
            },
            _ => {},
        }
    });

    app.borrow().set_status("Polling driver every 500ms...");
    nwg::dispatch_thread_events();
}

#[derive(Default)]
struct ValhallaApp {
    window: nwg::Window,
    list: nwg::ListView,
    refresh_btn: nwg::Button,
    clear_btn: nwg::Button,
    status: nwg::StatusBar,
    timer: nwg::AnimationTimer,
    event_count: u64,
}

impl ValhallaApp {
    fn build(app: &Rc<RefCell<ValhallaApp>>) -> Result<(), nwg::NwgError> {
        let mut a = app.borrow_mut();

        nwg::Window::builder()
            .size((920, 580))
            .position((300, 200))
            .title("Valhalla - Kernel Event Monitor")
            .build(&mut a.window)?;

        nwg::ListView::builder()
            .parent(&a.window)
            .size((880, 450))
            .position((20, 20))
            .list_style(nwg::ListViewStyle::Detailed)
            .build(&mut a.list)?;

        a.list.insert_column(nwg::InsertListViewColumn {
            index: Some(0),
            width: Some(90),
            fmt: None,
            text: Some("Time".to_string()),
        });
        a.list.insert_column(nwg::InsertListViewColumn {
            index: Some(1),
            width: Some(120),
            fmt: None,
            text: Some("Type".to_string()),
        });
        a.list.insert_column(nwg::InsertListViewColumn {
            index: Some(2),
            width: Some(70),
            fmt: None,
            text: Some("PID".to_string()),
        });
        a.list.insert_column(nwg::InsertListViewColumn {
            index: Some(3),
            width: Some(580),
            fmt: None,
            text: Some("Details".to_string()),
        });

        nwg::Button::builder()
            .text("Refresh Now")
            .parent(&a.window)
            .size((140, 35))
            .position((20, 490))
            .build(&mut a.refresh_btn)?;

        nwg::Button::builder()
            .text("Clear")
            .parent(&a.window)
            .size((100, 35))
            .position((170, 490))
            .build(&mut a.clear_btn)?;

        nwg::StatusBar::builder()
            .parent(&a.window)
            .build(&mut a.status)?;

        nwg::AnimationTimer::builder()
            .parent(&a.window)
            .interval(std::time::Duration::from_millis(POLL_INTERVAL_MS))
            .build(&mut a.timer)?;

        drop(a);
        Ok(())
    }

    fn poll_driver(&mut self) {
        if let Some(events) = read_events_from_driver() {
            for event in &events {
                self.add_event_row(event);
                self.event_count += 1;
            }
            if self.event_count > 0 {
                self.set_status(&format!("{} events collected", self.event_count));
            } else {
                self.set_status("No events yet. Generate activity to see events.");
            }
        } else {
            self.set_status("Cannot connect to \\\\.\\Valhalla. Is the driver running?");
        }
    }

    fn add_event_row(&mut self, item: &ItemInfo) {
        let (etype, pid, details, timestamp) = format_event(item);
        self.list
            .insert_items_row(None, &[&timestamp[..], &etype[..], &pid[..], &details[..]]);
    }

    fn clear_events(&mut self) {
        self.list.clear();
        self.event_count = 0;
        self.set_status("Cleared. Polling...");
    }

    fn set_status(&self, text: &str) {
        self.status.set_text(0, text);
    }
}

// ---------------------------------------------------------------------------
// CLI mode (original behavior, triggered by --cli)
// ---------------------------------------------------------------------------

fn run_cli() {
    println!("Valhalla client - CLI mode");

    unsafe {
        let h_file = CreateFileA(
            DEVICE_PATH.as_ptr(),
            GENERIC_READ,
            0,
            null_mut(),
            OPEN_EXISTING,
            0,
            0isize,
        ) as HANDLE;

        if h_file == INVALID_HANDLE_VALUE {
            print_last_error("Failed to open \\\\.\\Valhalla");
            return;
        }
        println!("Connected to driver.");

        let mut buffer = [0u8; 0x10000];
        let mut bytes: u32 = 0;

        let status = ReadFile(
            h_file,
            buffer.as_mut_ptr(),
            std::mem::size_of_val(&buffer) as u32,
            &mut bytes as *mut u32,
            null_mut(),
        );

        if status == 0 {
            print_last_error("ReadFile failed");
            return;
        }

        println!("Read {bytes} bytes.");
        if bytes != 0 {
            display_info(&buffer, bytes);
        }
    }
}

fn display_info(buffer: &[u8], size: u32) {
    let mut offset = 0;
    loop {
        if size == offset as u32 {
            break;
        }
        let item = unsafe { &*(buffer.as_ptr().add(offset) as *const ItemInfo) };
        println!("{item:?}");
        offset += size_of::<ItemInfo>();
    }
}

// ---------------------------------------------------------------------------
// Shared driver-reading logic
// ---------------------------------------------------------------------------

fn read_events_from_driver() -> Option<Vec<ItemInfo>> {
    unsafe {
        let h_file = CreateFileA(
            DEVICE_PATH.as_ptr(),
            GENERIC_READ,
            0,
            null_mut(),
            OPEN_EXISTING,
            0,
            0isize,
        ) as HANDLE;

        if h_file == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut buffer = [0u8; 0x10000];
        let mut bytes: u32 = 0;

        let status = ReadFile(
            h_file,
            buffer.as_mut_ptr(),
            std::mem::size_of_val(&buffer) as u32,
            &mut bytes as *mut u32,
            null_mut(),
        );

        if status == 0 || bytes == 0 {
            return Some(Vec::new());
        }

        let mut events = Vec::new();
        let mut offset = 0usize;
        while offset + size_of::<ItemInfo>() <= bytes as usize {
            let item = &*(buffer.as_ptr().add(offset) as *const ItemInfo);
            events.push(clone_item(item));
            offset += size_of::<ItemInfo>();
        }
        Some(events)
    }
}

fn clone_item(item: &ItemInfo) -> ItemInfo {
    match item {
        ItemInfo::ProcessCreate {
            pid,
            parent_pid,
            command_line,
        } => ItemInfo::ProcessCreate {
            pid: *pid,
            parent_pid: *parent_pid,
            command_line: command_line.clone(),
        },
        ItemInfo::ProcessExit { pid } => ItemInfo::ProcessExit { pid: *pid },
        ItemInfo::ThreadCreate { pid, tid } => ItemInfo::ThreadCreate {
            pid: *pid,
            tid: *tid,
        },
        ItemInfo::ThreadExit { pid, tid } => ItemInfo::ThreadExit {
            pid: *pid,
            tid: *tid,
        },
        ItemInfo::ImageLoad {
            pid,
            load_address,
            image_size,
            image_file_name,
        } => ItemInfo::ImageLoad {
            pid: *pid,
            load_address: *load_address,
            image_size: *image_size,
            image_file_name: image_file_name.clone(),
        },
        ItemInfo::RegistrySetValue {
            pid,
            tid,
            key_name,
            data_type,
        } => ItemInfo::RegistrySetValue {
            pid: *pid,
            tid: *tid,
            key_name: key_name.clone(),
            data_type: *data_type,
        },
    }
}

fn format_event(item: &ItemInfo) -> (String, String, String, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let timestamp = format!(
        "{:02}:{:02}:{:02}",
        (now % 86400) / 3600,
        (now % 3600) / 60,
        now % 60
    );

    match item {
        ItemInfo::ProcessCreate {
            pid,
            parent_pid,
            command_line,
        } => (
            "Process Create".to_string(),
            pid.to_string(),
            format!("PID {pid} <- {parent_pid}  {}", command_line.as_str_safe()),
            timestamp,
        ),
        ItemInfo::ProcessExit { pid } => (
            "Process Exit".to_string(),
            pid.to_string(),
            format!("PID {pid} exited"),
            timestamp,
        ),
        ItemInfo::ThreadCreate { pid, tid } => (
            "Thread Create".to_string(),
            pid.to_string(),
            format!("TID {tid} in PID {pid}"),
            timestamp,
        ),
        ItemInfo::ThreadExit { pid, tid } => (
            "Thread Exit".to_string(),
            pid.to_string(),
            format!("TID {tid} in PID {pid}"),
            timestamp,
        ),
        ItemInfo::ImageLoad {
            pid,
            load_address,
            image_size,
            image_file_name,
        } => (
            "Image Load".to_string(),
            pid.to_string(),
            format!(
                "PID {pid} @ 0x{:x} ({} bytes)  {}",
                load_address,
                image_size,
                image_file_name.as_str_safe()
            ),
            timestamp,
        ),
        ItemInfo::RegistrySetValue {
            pid,
            tid,
            key_name,
            data_type,
        } => (
            "Registry Set".to_string(),
            pid.to_string(),
            format!(
                "PID {pid} TID {tid} type={data_type}  {}",
                key_name.as_str_safe()
            ),
            timestamp,
        ),
    }
}

/// Helper trait to safely extract a Rust string from StringBuff.
trait StrSafe {
    fn as_str_safe(&self) -> String;
}

impl StrSafe for common::StringBuff {
    fn as_str_safe(&self) -> String {
        let v = self.as_bytes();
        let end = v.iter().position(|&b| b == 0).unwrap_or(v.len());
        String::from_utf8_lossy(&v[..end]).trim_end().to_string()
    }
}
