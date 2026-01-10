use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use if_addrs::Interface;
use tokio::sync::RwLock;

pub struct IfMonitor {
    interfaces: Arc<RwLock<Vec<Interface>>>,
	on_added: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>>,
	on_removed: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>>,
    shutdown: Arc<AtomicBool>,
}

impl IfMonitor {
    pub fn new() -> std::io::Result<Self> {
        let interfaces = Arc::new(RwLock::new(if_addrs::get_if_addrs()?));

        let on_added: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>> = Arc::new(Mutex::new(None));
        let on_removed: Arc<Mutex<Option<Box<dyn Fn(Interface) + Send + 'static>>>> = Arc::new(Mutex::new(None));

        let interfaces_clone = interfaces.clone();
        let on_added_clone = on_added.clone();
        let on_removed_clone = on_removed.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
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
                                log::info!("Interface added: {:?}", iface);
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
                                log::info!("Interface removed: {:?}", iface);
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

        Ok(IfMonitor {
            interfaces,
			on_added,
			on_removed,
            shutdown,
        })
    }

    pub async fn get_interfaces(&self) -> Vec<Interface> {
        let interfaces = self.interfaces.read().await;
        interfaces.clone()
    }

	pub fn on_added<F>(&self, callback: F)
	where
		F: Fn(Interface) + Send + 'static,
	{
		if let Ok(mut guard) = self.on_added.lock() {
			*guard = Some(Box::new(callback));
		}
	}

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
        self.shutdown.store(true, Ordering::Relaxed);
    }
}