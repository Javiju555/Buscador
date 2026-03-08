use ashpd::desktop::global_shortcuts::{
    Activated, BindShortcutsOptions, GlobalShortcuts, NewShortcut,
};
use ashpd::desktop::{CreateSessionOptions, Session};
use futures_lite::StreamExt;
use std::sync::Arc;

const SHORTCUT_ID: &str = "toggle-launcher";

pub fn spawn_portal_shortcut_listener<F>(on_activate: F)
where
    F: Fn() + Send + Sync + 'static,
{
    let on_activate = Arc::new(on_activate);
    std::thread::Builder::new()
        .name("portal-shortcut".into())
        .spawn(move || {
            let rt: tokio::runtime::Runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    log::error!("No se pudo crear runtime tokio para portal shortcuts: {error}");
                    return;
                }
            };

            rt.block_on(async move {
                if let Err(error) = run_portal_listener(on_activate).await {
                    log::error!("Error en portal shortcut listener: {error}");
                }
            });
        })
        .ok();
}

async fn run_portal_listener(
    on_activate: Arc<dyn Fn() + Send + Sync + 'static>,
) -> ashpd::Result<()> {
    let portal: GlobalShortcuts = GlobalShortcuts::new().await?;
    log::info!(
        "Portal GlobalShortcuts conectado (version {})",
        portal.version()
    );

    // Suscribirse a Activated ANTES de bind para no perder ningun signal
    let mut stream = std::pin::pin!(portal.receive_activated().await?);
    log::info!("Suscripcion a señal Activated lista");

    let session: Session<GlobalShortcuts> = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    log::info!("Sesion de global shortcuts creada");

    let shortcut =
        NewShortcut::new(SHORTCUT_ID, "Toggle Buscador launcher").preferred_trigger("CTRL+space");

    let request = portal
        .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
        .await?;

    match request.response() {
        Ok(response) => {
            log::info!("Shortcuts registrados via portal: {response:?}");
        }
        Err(error) => {
            log::error!("Error en la respuesta de bind_shortcuts: {error}");
            return Err(error);
        }
    }

    log::info!("Escuchando activaciones de shortcut via portal...");

    loop {
        match StreamExt::next(&mut stream).await {
            Some(activated) => {
                let activated: Activated = activated;
                let id: &str = activated.shortcut_id();
                log::info!("Portal shortcut activado: id={id}");
                if id == SHORTCUT_ID {
                    on_activate();
                }
            }
            None => {
                log::warn!("Stream de activacion de portal terminó inesperadamente");
                break;
            }
        }
    }

    Ok(())
}
