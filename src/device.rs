use crate::consts;
use crate::consts::PRODUCT_IDS;
use crate::keyboard::{k884x, k8890, Keyboard};

use anyhow::{anyhow, ensure, Context, Result};
use indoc::indoc;
use itertools::Itertools;
use log::debug;
use rusb::{Context as UsbCtx, Device, DeviceDescriptor, Direction, TransferType};
use rusb::UsbContext as _;

/// Finds the interface number and in/out endpoint addresses for a device
///
/// #Arguments
/// `device` - the USB device to probe
/// `interface_num` - restrict the search to this interface, if given
/// `endpoint_addr_out` - restrict the OUT endpoint match to this address, if given
/// `endpoint_addr_in` - restrict the IN endpoint match to this address, if given
///
pub fn find_interface_and_endpoint(
    device: &Device<UsbCtx>,
    interface_num: Option<u8>,
    endpoint_addr_out: Option<u8>,
    endpoint_addr_in: Option<u8>,
) -> Result<(u8, u8, u8)> {
    debug!("out: {endpoint_addr_out:?} in: {endpoint_addr_in:?}");
    let conf_desc = device
        .config_descriptor(0)
        .context("get config #0 descriptor")?;

    // Get the numbers of interfaces to explore
    let interface_nums = match interface_num {
        Some(iface_num) => vec![iface_num],
        None => conf_desc.interfaces().map(|iface| iface.number()).collect(),
    };

    // per usb spec, the max value for a usb endpoint is 7 bits (or 127)
    // so set the values to be invalid by default
    let mut out_if = 0xFF;
    let mut in_if = 0xFF;
    for iface_num in interface_nums {
        debug!("Probing interface {iface_num}");

        // Look for an interface with the given number
        let intf = conf_desc
            .interfaces()
            .find(|iface| iface_num == iface.number())
            .ok_or_else(|| {
                anyhow!(
                    "interface #{} not found, interface numbers:\n{:#?}",
                    iface_num,
                    conf_desc.interfaces().map(|i| i.number()).format(", ")
                )
            })?;

        // Check that it's a HID device
        let intf_desc = intf.descriptors().exactly_one().map_err(|_| {
            anyhow!(
                "only one interface descriptor is expected, got:\n{:#?}",
                intf.descriptors().format("\n")
            )
        })?;

        let descriptors = intf_desc.endpoint_descriptors();
        for endpoint in descriptors {
            // check packet size
            if endpoint.max_packet_size() != (consts::PACKET_SIZE - 1).try_into()? {
                continue;
            }

            debug!("==> {:?} direction: {:?}", endpoint, endpoint.direction());
            if endpoint.transfer_type() == TransferType::Interrupt
                && endpoint.direction() == Direction::Out
            {
                if let Some(ea) = endpoint_addr_out {
                    if endpoint.address() == ea {
                        debug!("Found OUT endpoint {endpoint:?}");
                        out_if = endpoint.address();
                    }
                } else {
                    debug!("Found OUT endpoint {endpoint:?}");
                    out_if = endpoint.address();
                }
            }
            if endpoint.transfer_type() == TransferType::Interrupt
                && endpoint.direction() == Direction::In
            {
                if let Some(ea) = endpoint_addr_in {
                    if endpoint.address() == ea {
                        debug!("Found IN endpoint {endpoint:?}");
                        in_if = endpoint.address();
                    }
                } else {
                    debug!("Found IN endpoint {endpoint:?}");
                    in_if = endpoint.address();
                }
            }
        }
        debug!("ep OUT addr: 0x{out_if:02x} ep IN addr: 0x{in_if:02x}");
        if out_if < 0xFF && in_if < 0xFF {
            return Ok((iface_num, out_if, in_if));
        } else if out_if < 0xFF {
            return Ok((iface_num, out_if, 0xFF));
        }
    }

    Err(anyhow!("No valid interface/endpoint combination found!"))
}

/// Finds a connected macropad device matching the given vendor/product id
///
/// #Arguments
/// `vid` - vendor id to search for
/// `pid` - restrict the search to this product id, if given
///
pub fn find_device(vid: u16, pid: Option<u16>) -> Result<(Device<UsbCtx>, DeviceDescriptor, u16)> {
    debug!("vid: 0x{vid:02x}");
    if let Some(prod_id) = pid {
        debug!("pid: 0x{prod_id:02x}");
    } else {
        debug!("pid: None");
    }
    let options = vec![
        #[cfg(windows)]
        rusb::UsbOption::use_usbdk(),
    ];
    let usb_context = UsbCtx::with_options(&options)?;

    let mut found = vec![];
    for device in usb_context.devices().context("get USB device list")?.iter() {
        let desc = device.device_descriptor().context("get USB device info")?;
        debug!(
            "Bus {:03} Device {:03} ID {:04x}:{:04x}",
            device.bus_number(),
            device.address(),
            desc.vendor_id(),
            desc.product_id()
        );
        let product_id = desc.product_id();

        if desc.vendor_id() == vid {
            if let Some(prod_id) = pid {
                if PRODUCT_IDS.contains(&prod_id) {
                    found.push((device, desc, product_id));
                }
            } else {
                found.push((device, desc, product_id));
            }
        }
    }

    match found.len() {
        0 => Err(anyhow!(
            "macropad device not found. Use --vendor-id and --product-id to override defaults"
        )),
        1 => Ok(found.pop().unwrap()),
        _ => {
            let mut addresses = vec![];
            for (device, _desc, _product_id) in found {
                let address = (device.bus_number(), device.address());
                addresses.push(address);
            }

            Err(anyhow!(
                indoc! {"
                Several compatible devices are found.
                Unfortunately, this model of keyboard doesn't have serial number.
                So specify USB address using --address option.

                Addresses:
                {}
            "},
                addresses
                    .iter()
                    .map(|(bus, addr)| format!("{bus}:{addr}"))
                    .join("\n")
            ))
        }
    }
}

/// Opens a connected macropad device and returns a handle implementing the [`Keyboard`] trait
///
/// #Arguments
/// `vendor_id` - vendor id to search for
/// `product_id` - restrict the search to this product id, if given
/// `interface_number` - restrict the interface search to this number, if given
/// `out_endpoint_address` - restrict the OUT endpoint match to this address, if given
/// `in_endpoint_address` - restrict the IN endpoint match to this address, if given
///
pub fn open_keyboard(
    vendor_id: u16,
    product_id: Option<u16>,
    interface_number: Option<u8>,
    out_endpoint_address: Option<u8>,
    in_endpoint_address: Option<u8>,
) -> Result<Box<dyn Keyboard>> {
    // Find USB device based on the product id
    let (device, desc, id_product) =
        find_device(vendor_id, product_id).context("find USB device")?;

    ensure!(
        desc.num_configurations() == 1,
        "only one device configuration is expected"
    );

    // Find correct endpoint
    let (intf_num, endpt_addr_out, endpt_addr_in) = find_interface_and_endpoint(
        &device,
        interface_number,
        out_endpoint_address,
        in_endpoint_address,
    )?;

    // Open device.
    let handle = device.open().context("open USB device")?;
    let _ = handle.set_auto_detach_kernel_driver(true);
    handle
        .claim_interface(intf_num)
        .context("claim interface")?;

    match id_product {
        0x8840 | 0x8842 => {
            k884x::Keyboard884x::new(Some(handle), endpt_addr_out, endpt_addr_in, id_product)
                .map(|v| Box::new(v) as Box<dyn Keyboard>)
        }
        0x8890 => k8890::Keyboard8890::new(Some(handle), endpt_addr_out)
            .map(|v| Box::new(v) as Box<dyn Keyboard>),
        _ => unreachable!("This shouldn't happen!"),
    }
}
