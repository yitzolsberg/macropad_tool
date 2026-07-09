mod options;
mod parse;

use macropad_tool::decoder::Decoder;
use macropad_tool::keyboard::{
    Keyboard, MediaCode, Modifier, MouseAction, MouseButton, WellKnownCode,
};
use macropad_tool::{consts, find_device, keyboard, mapping};

use crate::options::Options;
use crate::options::{Command, LedCommand};

use anyhow::{anyhow, Result};
use itertools::Itertools;
use keyboard::LedColor;
use log::debug;
use mapping::Mapping;

use anyhow::Context as _;
use clap::Parser as _;
use strum::EnumMessage as _;
use strum::IntoEnumIterator as _;

fn main() -> Result<()> {
    env_logger::init();
    let options = Options::parse();
    debug!("options: {:?}", options.devel_options);

    match &options.command {
        Command::ShowKeys => {
            println!("Modifiers: ");
            for m in Modifier::iter() {
                println!(" - {}", m.get_serializations().iter().join(" / "));
            }

            println!();
            println!("Keys:");
            for c in WellKnownCode::iter() {
                println!(" - {c}");
            }

            println!();
            println!("Custom key syntax (use decimal code): <110>");

            println!();
            println!("Media keys:");
            for c in MediaCode::iter() {
                println!(" - {}", c.get_serializations().iter().join(" / "));
            }

            println!();
            println!("Mouse actions:");
            println!(" - {}", MouseAction::WheelDown);
            println!(" - {}", MouseAction::WheelUp);
            for b in MouseButton::iter() {
                println!(" - {b}");
            }
        }

        Command::Validate {
            config_file,
            product_id,
            device_connected,
        } => {
            if *device_connected {
                debug!("validating with connected device");
                if let Ok(device) = find_device(consts::VENDOR_ID, None) {
                    // read the config for buttons/knobs and validate against file
                    if device.2 != 0x8890 {
                        // 0x8890 does not support reading configuration
                        let mut keyboard = open_keyboard(&options).context("opening keyboard")?;
                        let mut buf = vec![0; consts::READ_BUF_SIZE.into()];

                        // get the type of device
                        keyboard.send(&keyboard.device_type())?;
                        let bytes_read = keyboard.recieve(&mut buf)?;
                        if bytes_read == 0 {
                            return Err(anyhow!(
                                "Unable to read from device to validate mappings. Please use -p option instead to specify your device."
                            ));
                        }
                        let device_info = Decoder::get_device_info(&buf);
                        debug!(
                            "keys: {} encoders: {}",
                            device_info.num_keys, device_info.num_encoders
                        );

                        let macropad =
                            Mapping::read(config_file).context("reading configuration file")?;
                        if device_info.num_keys != macropad.device.rows * macropad.device.cols {
                            return Err(anyhow!(
                                "Number of keys specified in config does not match device"
                            ));
                        }
                        if device_info.num_encoders != macropad.device.knobs {
                            return Err(anyhow!(
                                "Number of knobs specified in config does not match device"
                            ));
                        }
                    }
                    Mapping::validate(config_file, Some(device.2))
                        .context("validating configuration file with connected device")?;
                    println!("config is valid 👌")
                } else {
                    return Err(anyhow!(
                        "Unable to find connected device with vendor id: 0x{:02x}",
                        consts::VENDOR_ID
                    ));
                }
            } else if let Some(pid) = product_id {
                debug!("validating with supplied product id 0x{pid:02x}");
                Mapping::validate(config_file, Some(*pid))
                    .context("validating configuration file against specified product id")?;
                println!("config is valid 👌")
            } else {
                // load and validate mapping
                println!("validating general ron formatting - unable to do more granular checking; use -p option to check against device");
                Mapping::validate(config_file, None)
                    .context("generic validation of configuration file")?;
                println!("config is valid 👌")
            }
        }

        Command::Program { config_file } => {
            let config = Mapping::read(config_file).context("reading configuration file")?;
            let mut keyboard = open_keyboard(&options).context("opening keyboard")?;
            keyboard.program(&config).context("programming macropad")?;
            println!("successfully programmed device");
        }

        Command::Led(LedCommand {
            index,
            layer,
            led_color,
        }) => {
            let mut keyboard = open_keyboard(&options).context("opening keyboard")?;

            // color is not supported on 0x8890 so don't require one to be passed
            let color = if led_color.is_some() {
                led_color.unwrap()
            } else {
                LedColor::Red
            };
            keyboard
                .set_led(*index, *layer, color)
                .context("programming LED on macropad")?;
        }

        Command::Read { layer } => {
            debug!("dev options: {:?}", options.devel_options);
            let mut keyboard = open_keyboard(&options).context("opening keyboard")?;
            let macropad_config = keyboard
                .read_macropad_config(layer)
                .context("reading macropad configuration")?;
            Mapping::print(macropad_config);
        }
    }

    Ok(())
}

fn open_keyboard(options: &Options) -> Result<Box<dyn Keyboard>> {
    macropad_tool::open_keyboard(
        options.devel_options.vendor_id,
        options.devel_options.product_id,
        options.devel_options.interface_number,
        options.devel_options.out_endpoint_address,
        options.devel_options.in_endpoint_address,
    )
}
