use poise::serenity_prelude as serenity;
use rand::{distributions::Alphanumeric, Rng};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;

// 1. Estructura para leer el JSON de la API de Habbo
#[derive(Deserialize, Debug)]
struct HabboProfile {
    motto: Option<String>,
    #[serde(rename = "profileVisible")]
    profile_visible: bool,
}

pub struct Data {
    pending_verifications: Arc<Mutex<HashMap<u64, (String, String)>>>,
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[poise::command(slash_command)]
async fn verificar(
    ctx: Context<'_>,
    #[description = "Tu nombre de usuario en Habbo"] habbo_name: String,
) -> Result<(), Error> {
    
    // 1. Limpiamos el nombre: quitamos espacios y el '@' por si el usuario lo pone por costumbre
    let clean_habbo_name = habbo_name.trim().trim_start_matches('@').to_string();

    let random_string: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();
    let code = format!("HBB-{}", random_string.to_uppercase());

    let button_id = format!("btn_verify_{}", ctx.id());

    let msg = format!(
        "¡Hola! Para vincular tu cuenta de Habbo (**{}**), copia y pega este código en tu Misión (Motto) dentro del juego:\n\n`{}`\n\nUna vez lo hayas guardado, haz clic en el botón de abajo para verificar.",
        clean_habbo_name, code
    );

    let components = vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(&button_id)
            .label("✅ Ya puse el código, Verificar")
            .style(serenity::ButtonStyle::Success),
    ])];

    let reply = poise::CreateReply::default()
        .content(msg)
        .components(components);

    ctx.send(reply).await?;

    while let Some(mci) = serenity::ComponentInteractionCollector::new(ctx)
        .author_id(ctx.author().id)
        .channel_id(ctx.channel_id())
        .timeout(Duration::from_secs(600))
        .filter({
            let button_id = button_id.clone();
            move |mci| mci.data.custom_id == button_id
        })
        .await
    {
        mci.create_response(ctx, serenity::CreateInteractionResponse::Defer(
            serenity::CreateInteractionResponseMessage::new().ephemeral(true)
        )).await?;

        let url = format!("https://www.habbo.es/api/public/users?name={}", clean_habbo_name);
        
        // 2. Creamos un cliente HTTP que simula ser un navegador web real (Chrome) para evitar bloqueos
        let client = reqwest::Client::new();
        match client.get(&url)
            .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .send()
            .await 
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    let profile: HabboProfile = resp.json().await?;
                    
                    if !profile.profile_visible {
                        let _ = mci.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                            .content("❌ **Error:** Tu perfil de Habbo está en **privado**. Hazlo público en tus ajustes del juego para poder leer tu misión.")).await;
                        continue;
                    }

                    if let Some(motto) = profile.motto {
                        if motto.contains(&code) {
                            
                            // --- INICIO DE LA LÓGICA DE ROLES Y APODOS ---
                            let _ = mci.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                .content(format!("🎉 **¡Verificación exitosa!** Tu cuenta de Discord ahora está vinculada a **{}**.", clean_habbo_name))).await;
                            
                            if let Some(guild_id) = ctx.guild_id() {
                                let user_id = ctx.author().id;
                                
                                // Cambiamos el apodo
                                let edit_member = serenity::EditMember::new().nickname(clean_habbo_name.clone());
                                let _ = guild_id.edit_member(ctx, user_id, edit_member).await;
                                
                                // Leemos el ID del rol desde el .env
                                let role_id_str = std::env::var("VERIFIED_ROLE_ID").unwrap_or_else(|_| "0".to_string());
                                let role_id_num = role_id_str.parse::<u64>().unwrap_or(0);
                                
                                if role_id_num != 0 {
                                    let role_id = serenity::RoleId::new(role_id_num);
                                    let _ = ctx.http().add_member_role(guild_id, user_id, role_id, Some("Verificación Habbo exitosa")).await;
                                } else {
                                    println!("Advertencia: VERIFIED_ROLE_ID no configurado o inválido en .env");
                                }
                            }
                            // --- FIN DE LA LÓGICA DE ROLES Y APODOS ---

                            break; 
                        } else {
                            let _ = mci.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                .content(format!("⚠️ La misión actual de tu perfil es `{}`. Asegúrate de poner exactamente `{}` y guarda los cambios en el juego.", motto, code))).await;
                        }
                    } else {
                        let _ = mci.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                            .content("⚠️ Tu misión de Habbo está vacía. Pon el código y vuelve a intentarlo.")).await;
                    }

                } else if resp.status().as_u16() == 404 {
                    let _ = mci.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                        .content("❌ No se encontró el usuario en Habbo. Revisa que el nombre esté bien escrito.")).await;
                    break;
                } else {
                    let _ = mci.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                        .content(format!("⚠️ Error del servidor de Habbo (Código: {}). Intenta en unos minutos.", resp.status().as_u16()))).await;
                }
            },
            Err(e) => {
                let _ = mci.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                    .content(format!("⚠️ Error de conexión interno del bot: {}", e))).await;
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN")
        .expect("Falta la variable de entorno DISCORD_TOKEN en el archivo .env");

    let intents = serenity::GatewayIntents::non_privileged() 
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![verificar()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                println!("¡Bot de Habbo conectado y comandos registrados!");
                Ok(Data {
                    pending_verifications: Arc::new(Mutex::new(HashMap::new())),
                })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .unwrap();

    if let Err(why) = client.start().await {
        println!("Error crítico del cliente: {:?}", why);
    }
}