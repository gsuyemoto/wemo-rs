extern crate eframe;
extern crate time;
extern crate wemo;

use eframe::{egui, App, CreationContext, Frame, NativeOptions};
use egui::{Color32, Context, RichText, Ui};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration as StdDuration;
use time::Duration;
use wemo::{DeviceSearch, Switch};

// Structure to hold device information
struct DeviceInfo {
    name: String,
    ip_address: std::net::IpAddr,
    port: u16,
    serial_number: String,
    state: Option<wemo::WemoState>,
    status_message: String,
}

// Main application structure
struct WemoApp {
    devices: Arc<Mutex<HashMap<String, DeviceInfo>>>,
    scanning: bool,
    refresh_interval: u64,
    last_refresh: std::time::Instant,
}

impl WemoApp {
    fn new(_cc: &CreationContext<'_>) -> Self {
        // Initialize the device map with an empty HashMap
        let devices = Arc::new(Mutex::new(HashMap::new()));

        // Start an initial scan
        let devices_clone = Arc::clone(&devices);
        thread::spawn(move || {
            Self::scan_for_devices(devices_clone);
        });

        Self {
            devices,
            scanning: true,
            refresh_interval: 10, // Refresh every 10 seconds
            last_refresh: std::time::Instant::now(),
        }
    }

    // Find all WeMo devices on the network
    fn scan_for_devices(devices: Arc<Mutex<HashMap<String, DeviceInfo>>>) {
        // Clear existing devices
        devices.lock().unwrap().clear();

        let mut search = DeviceSearch::new();
        let results = search.search(5_000); // 5 second timeout

        let mut devices_map = devices.lock().unwrap();

        for (key, device) in results.iter() {
            let switch = Switch::from_dynamic_ip_and_port(device.ip_address, device.port);
            let name = switch.name();

            // Get the initial state
            let state = switch.get_state_with_retry(Duration::seconds(3)).ok();
            let status_message = match &state {
                Some(s) => if s.is_on() { "ON" } else { "OFF" }.to_string(),
                None => "Unknown".to_string(),
            };

            // Add to our device map
            devices_map.insert(
                key.clone(),
                DeviceInfo {
                    name,
                    ip_address: device.ip_address,
                    port: device.port,
                    serial_number: device.serial_number.clone(),
                    state,
                    status_message,
                },
            );
        }
    }

    // Refresh the state of all devices
    fn refresh_states(&mut self) {
        let devices_clone = Arc::clone(&self.devices);

        thread::spawn(move || {
            let devices_map = devices_clone.lock().unwrap();

            for (key, device_info) in devices_map.iter() {
                let switch =
                    Switch::from_dynamic_ip_and_port(device_info.ip_address, device_info.port);

                // Use a separate thread for each device to avoid blocking
                let key_clone = key.clone();
                let devices_clone_inner = Arc::clone(&devices_clone);

                thread::spawn(move || {
                    let state = switch.get_state_with_retry(Duration::seconds(3)).ok();

                    // Update device state
                    let mut devices_map = devices_clone_inner.lock().unwrap();
                    if let Some(device) = devices_map.get_mut(&key_clone) {
                        device.state = state;
                        if let Some(s) = &device.state {
                            device.status_message =
                                if s.is_on() { "ON" } else { "OFF" }.to_string();
                        } else {
                            device.status_message = "Unknown".to_string();
                        }
                    }
                });
            }
        });
    }

    // Toggle a device on or off
    fn toggle_device(&self, device_info: &DeviceInfo, turn_on: bool) {
        let ip = device_info.ip_address;
        let port = device_info.port;

        thread::spawn(move || {
            let switch = Switch::from_dynamic_ip_and_port(ip, port);
            let timeout = Duration::seconds(5);

            if turn_on {
                let _ = switch.turn_on_with_retry(timeout);
            } else {
                let _ = switch.turn_off_with_retry(timeout);
            }

            // Sleep a moment to let the device state change
            thread::sleep(StdDuration::from_millis(500));
        });
    }
}

impl App for WemoApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        // Auto-refresh device states
        let now = std::time::Instant::now();
        if now.duration_since(self.last_refresh).as_secs() >= self.refresh_interval {
            self.refresh_states();
            self.last_refresh = now;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WeMo Device Controller");

            ui.add_space(10.0);

            // Scan button
            if self.scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Scanning for devices...");
                });
            } else if ui.button("Scan for Devices").clicked() {
                self.scanning = true;
                let devices_clone = Arc::clone(&self.devices);

                thread::spawn(move || {
                    WemoApp::scan_for_devices(devices_clone);
                });
            }

            ui.add_space(10.0);

            // Refresh interval slider
            ui.horizontal(|ui| {
                ui.label("Refresh interval:");
                ui.add(egui::Slider::new(&mut self.refresh_interval, 5..=60).suffix(" sec"));
            });

            ui.add_space(10.0);

            // Display devices section
            ui.heading("Devices");
            ui.separator();

            let devices = self.devices.lock().unwrap();

            if devices.is_empty() {
                ui.label(
                    "No devices found. Click \"Scan for Devices\" to search for WeMo devices.",
                );
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (id, device_info) in devices.iter() {
                        self.render_device_ui(ui, id, device_info);
                    }
                });
            }
        });

        // Request continuous repainting
        ctx.request_repaint();
    }
}

impl WemoApp {
    // Render a single device UI element
    fn render_device_ui(&self, ui: &mut Ui, id: &str, device_info: &DeviceInfo) {
        ui.push_id(id, |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    // Device name and status
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&device_info.name).size(18.0).strong());
                        ui.label(format!("Serial: {}", &device_info.serial_number));
                        ui.label(format!(
                            "IP: {}:{}",
                            &device_info.ip_address, &device_info.port
                        ));

                        // Status with color
                        let status_text = format!("Status: {}", &device_info.status_message);
                        let status_color = match device_info.status_message.as_str() {
                            "ON" => Color32::GREEN,
                            "OFF" => Color32::RED,
                            _ => Color32::GRAY,
                        };
                        ui.label(RichText::new(status_text).color(status_color).strong());
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // On/Off buttons
                        let is_on = device_info.state.as_ref().map_or(false, |s| s.is_on());

                        // OFF button
                        let off_button = ui.add_enabled(
                            is_on,
                            egui::Button::new(RichText::new("Turn OFF").color(Color32::WHITE))
                                .fill(Color32::RED),
                        );

                        if off_button.clicked() {
                            self.toggle_device(device_info, false);
                        }

                        // ON button
                        let on_button = ui.add_enabled(
                            !is_on,
                            egui::Button::new(RichText::new("Turn ON").color(Color32::WHITE))
                                .fill(Color32::GREEN),
                        );

                        if on_button.clicked() {
                            self.toggle_device(device_info, true);
                        }
                    });
                });
            });
            ui.add_space(4.0);
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = NativeOptions {
        // initial_window_size: Some(egui::vec2(600.0, 500.0)),
        ..Default::default()
    };

    eframe::run_native(
        "WeMo Device Controller",
        options,
        Box::new(|cc| Box::new(WemoApp::new(cc))),
    )
}
