use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::Duration,
};

use log::error;

use crate::document::TextNodeId;

pub(crate) struct InterThreadMessage {
    pub(crate) node_id: TextNodeId,
    pub(crate) content: String,
}

pub(crate) fn spawn(
    id: TextNodeId,
    content: &str,
    tx: Sender<InterThreadMessage>,
    shutdown: Arc<AtomicBool>,
) {
    let content = content.to_string();
    _ = thread::spawn(move || thread_fn(id, content, tx, shutdown));
}

fn thread_fn(
    id: TextNodeId,
    mut content: String,
    tx: Sender<InterThreadMessage>,
    shutdown: Arc<AtomicBool>,
) {
    let fname = std::env::temp_dir().join(format!("canva-note-{}.md", id.0));
    fs::write(&fname, &content).unwrap();
    let mut modified = fs::metadata(&fname).unwrap().modified().unwrap();

    let mut proc = std::process::Command::new("wezterm")
        .args(["start", "--always-new-process", "hx"])
        .arg(&fname)
        .spawn()
        .unwrap();

    let mut results = vec![];
    let res = (|| {
        loop {
            let current_modified = fs::metadata(&fname)?.modified()?;
            if current_modified != modified {
                modified = current_modified;
                let current_content = fs::read_to_string(&fname)?;
                if current_content != content {
                    content = current_content.clone();
                    tx.send(InterThreadMessage {
                        node_id: id,
                        content: current_content,
                    })?;
                }
            }

            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            if proc.try_wait()?.is_some() {
                break;
            }

            thread::sleep(Duration::from_millis(250));
        }
        anyhow::Ok(())
    })();
    results.push(res);

    results.push(fs::remove_file(&fname).map_err(anyhow::Error::from));
    results.push(proc.kill().map_err(anyhow::Error::from));

    for res in results {
        if let Err(e) = res {
            error!("{e:?}");
        }
    }
}
