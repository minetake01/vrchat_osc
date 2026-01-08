use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use if_addrs::Interface;
use tokio::sync::{Mutex, RwLock};

pub struct IfMonitor {
    interfaces: Arc<RwLock<Vec<Interface>>>,
    notify_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<if_addrs::IfChangeType>>>,
    shutdown: Arc<AtomicBool>,
}

impl IfMonitor {
    pub fn new() -> std::io::Result<Self> {
        let interfaces = Arc::new(RwLock::new(if_addrs::get_if_addrs()?));

        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let interfaces_clone = interfaces.clone();
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
                        let _ = tx.send(detail.clone());

                        match detail {
                            if_addrs::IfChangeType::Added(iface) => {
                                log::info!("Interface added: {:?}", iface);
                                let mut interfaces = interfaces_clone.blocking_write();
                                if !interfaces.contains(&iface) {
                                    interfaces.push(iface);
                                }
                            }
                            if_addrs::IfChangeType::Removed(iface) => {
                                log::info!("Interface removed: {:?}", iface);
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
            notify_rx: Arc::new(Mutex::new(rx)),
            shutdown,
        })
    }

    pub async fn get_interfaces(&self) -> Vec<Interface> {
        let interfaces = self.interfaces.read().await;
        interfaces.clone()
    }

    pub async fn recv_change(&self) -> Option<if_addrs::IfChangeType> {
        self.notify_rx.lock().await.recv().await
    }
}

impl Drop for IfMonitor {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}