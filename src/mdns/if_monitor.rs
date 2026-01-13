use std::sync::{Arc, Mutex};
#[cfg(not(apple_or_bsd_or_illumos))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(apple_or_bsd_or_illumos))]
use std::time::Duration;

use if_addrs::Interface;
use tokio::sync::RwLock;

/// Monitors network interface changes and provides a way to react to them.
pub struct IfMonitor {
    /// List of currently available network interfaces.
    interfaces: Arc<RwLock<Vec<Interface>>>,
    /// Callback to be executed when a new interface is added.
    on_added: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>>,
    /// Callback to be executed when an interface is removed.
    on_removed: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>>,
    /// Flag to signal the background monitoring thread to stop.
    #[cfg(not(apple_or_bsd_or_illumos))]
    shutdown: Arc<AtomicBool>,
}

impl IfMonitor {
    /// Creates a new `IfMonitor` instance and starts a background thread to watch for interface changes.
    pub fn new() -> std::io::Result<Self> {
        let interfaces = Arc::new(RwLock::new(if_addrs::get_if_addrs()?));

        let on_added: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>> =
            Arc::new(Mutex::new(None));
        let on_removed: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>> =
            Arc::new(Mutex::new(None));

        #[cfg(not(apple_or_bsd_or_illumos))]
        let shutdown = Arc::new(AtomicBool::new(false));

        #[cfg(not(apple_or_bsd_or_illumos))]
        {
            let interfaces_clone = interfaces.clone();
            let on_added_clone = on_added.clone();
            let on_removed_clone = on_removed.clone();
            let shutdown_clone = shutdown.clone();

            std::thread::spawn(move || {
                let Ok(mut notifier) = if_addrs::IfChangeNotifier::new() else {
                    log::error!("Failed to initialize interface change notifier.");
                    log::warn!("Network configuration changes will not be automatically detected.");
                    return;
                };

                loop {
                    if shutdown_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Ok(details) = notifier.wait(Some(Duration::from_secs(1))) {
                        for detail in details {
                            match detail {
                                if_addrs::IfChangeType::Added(iface) => {
                                    log::debug!("Interface added: {:?}", iface);
                                    // Call the on_added callback if it exists
                                    if let Ok(callback_guard) = on_added_clone.lock() {
                                        if let Some(callback) = callback_guard.as_ref() {
                                            callback(iface.clone());
                                        }
                                    }
                                    // Update the interfaces list
                                    let mut interfaces = interfaces_clone.blocking_write();
                                    if !interfaces.contains(&iface) {
                                        interfaces.push(iface);
                                    }
                                }
                                if_addrs::IfChangeType::Removed(iface) => {
                                    log::debug!("Interface removed: {:?}", iface);
                                    // Call the on_removed callback if it exists
                                    if let Ok(callback_guard) = on_removed_clone.lock() {
                                        if let Some(callback) = callback_guard.as_ref() {
                                            callback(iface.clone());
                                        }
                                    }
                                    let mut interfaces = interfaces_clone.blocking_write();
                                    interfaces.retain(|i| i != &iface);
                                }
                            }
                        }
                    }
                }
            });
        }

        #[cfg(apple_or_bsd_or_illumos)]
        {
            log::warn!("Interface monitoring is currently not supported on Apple and BSD-based systems due to limitations in dependency crates.");
            log::warn!("Stable operation is not guaranteed on these platforms. We intend to support these systems in a future update.");
        }

        Ok(IfMonitor {
            interfaces,
            on_added,
            on_removed,
            #[cfg(not(apple_or_bsd_or_illumos))]
            shutdown,
        })
    }

    pub async fn get_interfaces(&self) -> Vec<Interface> {
        let interfaces = self.interfaces.read().await;
        interfaces.clone()
    }

    /// Registers a callback for when a new interface is added.
    pub fn on_added<F>(&self, callback: F)
    where
        F: Fn(Interface) + Send + 'static,
    {
        if let Ok(mut guard) = self.on_added.lock() {
            *guard = Some(Box::new(callback));
        }
    }

    /// Registers a callback for when an interface is removed.
    pub fn on_removed<F>(&self, callback: F)
    where
        F: Fn(Interface) + Send + 'static,
    {
        if let Ok(mut guard) = self.on_removed.lock() {
            *guard = Some(Box::new(callback));
        }
    }
}

impl Drop for IfMonitor {
    fn drop(&mut self) {
        #[cfg(not(apple_or_bsd_or_illumos))]
        self.shutdown.store(true, Ordering::Relaxed);
    }
}
