//! CoreBluetooth, on one serial queue.
//!
//! # The shape
//!
//! One delegate object implements all three CoreBluetooth protocols and owns
//! every Objective-C pointer in this crate. It is created on a serial dispatch
//! queue, it never leaves that queue, and CoreBluetooth delivers its callbacks
//! there. A repeating block on the same queue moves bytes between the L2CAP
//! streams and [`Shared`].
//!
//! Because nothing Objective-C is ever touched from another thread, this file
//! needs no `Send`/`Sync` impls and no argument about whether a `CBPeripheral`
//! may be shared. That is deliberate: the unsafe here is for talking to
//! CoreBluetooth, and it should not also have to cover threading.
//!
//! # Both roles, always
//!
//! Every node is a peripheral *and* a central. A node that only advertised
//! would never find anyone; a node that only scanned would never be found.
//! The two halves are independent — either can fail without the other.
//!
//! # Unverified
//!
//! Nothing in this file has been observed working. It compiles. Verifying it
//! needs two Macs, because a device does not discover its own advertisements,
//! and it cannot be put under `cargo test` because a process without an app
//! bundle is refused Bluetooth by TCC.

use crate::shared::{LinkId, Shared};
use crate::{Config, RadioError};
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, Message};
use objc2_core_bluetooth::{
    CBAdvertisementDataServiceUUIDsKey, CBAttributePermissions, CBCentralManager,
    CBCentralManagerDelegate, CBCharacteristic, CBCharacteristicProperties, CBL2CAPChannel,
    CBManagerState, CBMutableCharacteristic, CBMutableService, CBPeripheral, CBPeripheralDelegate,
    CBPeripheralManager, CBPeripheralManagerDelegate, CBService, CBUUID,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSInputStream, NSNumber, NSObject, NSObjectProtocol,
    NSStreamStatus, NSString,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// How many bytes are read from a stream per pump.
///
/// One L2CAP SDU is up to 64 KiB; this is read repeatedly until the stream is
/// empty, so the size is a buffer choice rather than a limit.
const READ_CHUNK: usize = 4096;

/// Objective-C objects owned by the delegate.
///
/// In a `RefCell` because CoreBluetooth callbacks are re-entrant in principle —
/// a delegate method can trigger another — and a borrow that outlives one
/// statement is a panic waiting for a busy room.
struct State {
    central: Option<Retained<CBCentralManager>>,
    peripheral_manager: Option<Retained<CBPeripheralManager>>,
    /// The PSM this node's published channel listens on, once CoreBluetooth
    /// has assigned one.
    psm: Option<u16>,
    /// Peripherals being connected to.
    ///
    /// **Retained deliberately.** CoreBluetooth does not hold a strong
    /// reference to a peripheral you are connecting to; drop it and the
    /// connection is abandoned with no callback and no error. It presents as
    /// "discovery works but nothing ever connects".
    peripherals: Vec<Retained<CBPeripheral>>,
    /// Open channels, by link.
    channels: HashMap<LinkId, Retained<CBL2CAPChannel>>,
    advertising: bool,
}

struct Ivars {
    shared: Arc<Shared>,
    config: Config,
    state: RefCell<State>,
}

define_class!(
    // SAFETY:
    // - NSObject imposes no subclassing requirements.
    // - This class overrides no superclass method, does not retain itself, and
    //   implements no Drop.
    #[unsafe(super(NSObject))]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    // ---- Central: find peers, read their PSM, open a channel to it ----
    unsafe impl CBCentralManagerDelegate for Delegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn central_did_update_state(&self, central: &CBCentralManager) {
            let state = unsafe { central.state() };
            if state == CBManagerState::PoweredOn {
                self.start_scanning(central);
            } else {
                // Reported rather than retried: "powered off" and "the user
                // declined" are things only the user can resolve, and a silent
                // retry loop would spend battery hiding that.
                self.ivars().shared.unavailable(describe_state(state));
            }
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn did_discover(
            &self,
            central: &CBCentralManager,
            peripheral: &CBPeripheral,
            _advertisement: &NSDictionary<NSString, AnyObjectRef>,
            _rssi: &NSNumber,
        ) {
            let already = {
                let state = self.ivars().state.borrow();
                state.peripherals.iter().any(|known| same_peer(known, peripheral))
            };
            if already {
                return;
            }

            let retained = peripheral.retain();
            unsafe { retained.setDelegate(Some(ProtocolObject::from_ref(self))) };
            self.ivars().state.borrow_mut().peripherals.push(retained.clone());

            tracing::debug!("discovered a node; connecting");
            unsafe { central.connectPeripheral_options(&retained, None) };
        }

        #[unsafe(method(centralManager:didConnectPeripheral:))]
        fn did_connect(&self, _central: &CBCentralManager, peripheral: &CBPeripheral) {
            let service = self.service_uuid();
            let services = NSArray::from_retained_slice(&[service]);
            unsafe { peripheral.discoverServices(Some(&services)) };
        }

        #[unsafe(method(centralManager:didFailToConnectPeripheral:error:))]
        fn did_fail_to_connect(
            &self,
            _central: &CBCentralManager,
            peripheral: &CBPeripheral,
            error: Option<&NSError>,
        ) {
            tracing::debug!(error = ?error.map(ToString::to_string), "connect failed");
            self.forget_peripheral(peripheral);
        }

        #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
        fn did_disconnect(
            &self,
            _central: &CBCentralManager,
            peripheral: &CBPeripheral,
            _error: Option<&NSError>,
        ) {
            self.forget_peripheral(peripheral);
        }
    }

    // ---- The peer we connected to: its services, its PSM, its channel ----
    unsafe impl CBPeripheralDelegate for Delegate {
        #[unsafe(method(peripheral:didDiscoverServices:))]
        fn did_discover_services(&self, peripheral: &CBPeripheral, error: Option<&NSError>) {
            if error.is_some() {
                return;
            }
            let Some(services) = (unsafe { peripheral.services() }) else {
                return;
            };
            let wanted = self.psm_uuid();
            for service in services.to_vec() {
                let characteristics = NSArray::from_retained_slice(&[wanted.clone()]);
                unsafe {
                    peripheral.discoverCharacteristics_forService(Some(&characteristics), &service);
                }
            }
        }

        #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
        fn did_discover_characteristics(
            &self,
            peripheral: &CBPeripheral,
            service: &CBService,
            error: Option<&NSError>,
        ) {
            if error.is_some() {
                return;
            }
            let Some(characteristics) = (unsafe { service.characteristics() }) else {
                return;
            };
            for characteristic in characteristics.to_vec() {
                unsafe { peripheral.readValueForCharacteristic(&characteristic) };
            }
        }

        #[unsafe(method(peripheral:didUpdateValueForCharacteristic:error:))]
        fn did_update_value(
            &self,
            peripheral: &CBPeripheral,
            characteristic: &CBCharacteristic,
            error: Option<&NSError>,
        ) {
            if error.is_some() {
                return;
            }
            let Some(data) = (unsafe { characteristic.value() }) else {
                return;
            };
            let bytes = data.to_vec();
            // Two bytes, big-endian, and refused rather than guessed at: a
            // wrong PSM opens a channel to whatever else is listening.
            let Ok(psm) = <[u8; 2]>::try_from(bytes.as_slice()) else {
                tracing::debug!(len = bytes.len(), "PSM characteristic was not two bytes");
                return;
            };
            let psm = u16::from_be_bytes(psm);
            tracing::debug!(psm, "opening an L2CAP channel to a node");
            unsafe { peripheral.openL2CAPChannel(psm) };
        }

        #[unsafe(method(peripheral:didOpenL2CAPChannel:error:))]
        fn peripheral_did_open_channel(
            &self,
            _peripheral: &CBPeripheral,
            channel: Option<&CBL2CAPChannel>,
            error: Option<&NSError>,
        ) {
            self.adopt_channel(channel, error);
        }
    }

    // ---- Peripheral: publish a channel, advertise its PSM, accept peers ----
    unsafe impl CBPeripheralManagerDelegate for Delegate {
        #[unsafe(method(peripheralManagerDidUpdateState:))]
        fn peripheral_manager_did_update_state(&self, manager: &CBPeripheralManager) {
            let state = unsafe { manager.state() };
            if state == CBManagerState::PoweredOn {
                // The channel first: its PSM is what the service has to
                // publish, so advertising before it exists would advertise a
                // service with nothing behind it.
                unsafe { manager.publishL2CAPChannelWithEncryption(false) };
            } else {
                self.ivars().shared.unavailable(describe_state(state));
            }
        }

        #[unsafe(method(peripheralManager:didPublishL2CAPChannel:error:))]
        fn did_publish_channel(
            &self,
            manager: &CBPeripheralManager,
            psm: u16,
            error: Option<&NSError>,
        ) {
            if let Some(error) = error {
                self.ivars()
                    .shared
                    .unavailable(format!("could not publish an L2CAP channel: {error}"));
                return;
            }

            self.ivars().state.borrow_mut().psm = Some(psm);
            tracing::info!(psm, "published an L2CAP channel");
            self.publish_service(manager, psm);
        }

        #[unsafe(method(peripheralManager:didAddService:error:))]
        fn did_add_service(
            &self,
            manager: &CBPeripheralManager,
            _service: &CBService,
            error: Option<&NSError>,
        ) {
            if let Some(error) = error {
                self.ivars()
                    .shared
                    .unavailable(format!("could not publish the service: {error}"));
                return;
            }
            self.start_advertising(manager);
        }

        #[unsafe(method(peripheralManager:didOpenL2CAPChannel:error:))]
        fn manager_did_open_channel(
            &self,
            _manager: &CBPeripheralManager,
            channel: Option<&CBL2CAPChannel>,
            error: Option<&NSError>,
        ) {
            self.adopt_channel(channel, error);
        }
    }
);

/// An untyped Objective-C reference, for dictionary values we do not read.
type AnyObjectRef = objc2::runtime::AnyObject;

impl Delegate {
    fn service_uuid(&self) -> Retained<CBUUID> {
        let string = NSString::from_str(&self.ivars().config.service_uuid);
        unsafe { CBUUID::UUIDWithString(&string) }
    }

    fn psm_uuid(&self) -> Retained<CBUUID> {
        let string = NSString::from_str(&self.ivars().config.psm_uuid);
        unsafe { CBUUID::UUIDWithString(&string) }
    }

    fn start_scanning(&self, central: &CBCentralManager) {
        let services = NSArray::from_retained_slice(&[self.service_uuid()]);
        // No `AllowDuplicates`: a repeat advertisement from a peer already
        // connected is pure wakeups, and this scan is filtered to one service
        // anyway.
        unsafe { central.scanForPeripheralsWithServices_options(Some(&services), None) };
        tracing::info!("scanning for nodes");
    }

    /// Exposes the PSM as a readable characteristic on the advertised service.
    fn publish_service(&self, manager: &CBPeripheralManager, psm: u16) {
        let value = NSData::with_bytes(&psm.to_be_bytes());
        let characteristic = unsafe {
            CBMutableCharacteristic::initWithType_properties_value_permissions(
                CBMutableCharacteristic::alloc(),
                &self.psm_uuid(),
                CBCharacteristicProperties::Read,
                // A cached value rather than a read handler: it never changes
                // while the process lives, and a static value needs no
                // `didReceiveReadRequest` round trip through this delegate.
                Some(&value),
                CBAttributePermissions::Readable,
            )
        };

        let service = unsafe {
            CBMutableService::initWithType_primary(
                CBMutableService::alloc(),
                &self.service_uuid(),
                true,
            )
        };
        let characteristics = NSArray::from_retained_slice(&[Retained::into_super(characteristic)]);
        unsafe { service.setCharacteristics(Some(&characteristics)) };
        unsafe { manager.addService(&service) };
    }

    fn start_advertising(&self, manager: &CBPeripheralManager) {
        if self.ivars().state.borrow().advertising {
            return;
        }

        let key = unsafe { CBAdvertisementDataServiceUUIDsKey };
        let services = NSArray::from_retained_slice(&[self.service_uuid()]);
        let value = Retained::into_super(Retained::into_super(services));
        let advertisement = NSDictionary::from_slices::<NSString>(&[key], &[&*value]);
        unsafe { manager.startAdvertising(Some(&advertisement)) };

        self.ivars().state.borrow_mut().advertising = true;
        tracing::info!("advertising");
    }

    /// Takes ownership of a newly opened channel, from either role.
    fn adopt_channel(&self, channel: Option<&CBL2CAPChannel>, error: Option<&NSError>) {
        if let Some(error) = error {
            tracing::debug!(%error, "an L2CAP channel failed to open");
            return;
        }
        let Some(channel) = channel else {
            return;
        };

        let (Some(input), Some(output)) =
            (unsafe { channel.inputStream() }, unsafe { channel.outputStream() })
        else {
            tracing::debug!("an L2CAP channel opened without streams");
            return;
        };

        // Opened, not scheduled on a run loop: this crate polls both streams
        // from the same serial queue the delegate runs on, so there is no run
        // loop to schedule them on and none is needed.
        input.open();
        output.open();

        let link = self.ivars().shared.open();
        self.ivars()
            .state
            .borrow_mut()
            .channels
            .insert(link, channel.retain());
        tracing::info!(link, "link up");
    }

    fn forget_peripheral(&self, peripheral: &CBPeripheral) {
        self.ivars()
            .state
            .borrow_mut()
            .peripherals
            .retain(|known| !same_peer(known, peripheral));
    }

    /// Moves bytes in both directions, once.
    fn pump(&self) {
        for link in self.ivars().shared.drain_closing() {
            self.tear_down(link);
        }

        let links: Vec<LinkId> = self.ivars().state.borrow().channels.keys().copied().collect();

        for link in links {
            if !self.read_from(link) || !self.write_to(link) {
                self.tear_down(link);
            }
        }
    }

    /// Drains everything waiting on a link's input stream.
    ///
    /// Returns whether the link is still healthy.
    fn read_from(&self, link: LinkId) -> bool {
        let Some(channel) = self.channel(link) else {
            return false;
        };
        let Some(input) = (unsafe { channel.inputStream() }) else {
            return false;
        };

        if is_finished(&input) {
            return false;
        }

        let mut buffer = [0u8; READ_CHUNK];
        while input.hasBytesAvailable() {
            let read: isize = unsafe { msg_send![&*input, read: buffer.as_mut_ptr(), maxLength: READ_CHUNK] };
            match read {
                // Zero is end of stream, negative is an error. Both mean the
                // link is over; neither is worth distinguishing to a caller
                // whose only response is to drop it.
                ..=0 => return false,
                count => {
                    // The cast is bounded by READ_CHUNK, which the call above
                    // is not permitted to exceed.
                    let count = count as usize;
                    self.ivars()
                        .shared
                        .received(link, buffer[..count.min(READ_CHUNK)].to_vec());
                }
            }
        }
        true
    }

    /// Writes what is queued for a link, as far as the stream will take it.
    ///
    /// Returns whether the link is still healthy.
    fn write_to(&self, link: LinkId) -> bool {
        let Some(channel) = self.channel(link) else {
            return false;
        };
        let Some(output) = (unsafe { channel.outputStream() }) else {
            return false;
        };

        while output.hasSpaceAvailable() {
            let Some(chunk) = self.ivars().shared.take(link) else {
                return true;
            };
            let written: isize =
                unsafe { msg_send![&*output, write: chunk.as_ptr(), maxLength: chunk.len()] };
            match written {
                ..=0 => return false,
                count => {
                    // A short write is legal. Putting the tail back is what
                    // stops a half-written frame desynchronising the peer's
                    // length prefix — which reads as corruption, not as
                    // backpressure.
                    let count = (count as usize).min(chunk.len());
                    if count < chunk.len() {
                        self.ivars().shared.put_back(link, chunk[count..].to_vec());
                        return true;
                    }
                }
            }
        }
        true
    }

    fn channel(&self, link: LinkId) -> Option<Retained<CBL2CAPChannel>> {
        self.ivars().state.borrow().channels.get(&link).cloned()
    }

    fn tear_down(&self, link: LinkId) {
        let channel = self.ivars().state.borrow_mut().channels.remove(&link);
        if let Some(channel) = channel {
            if let Some(input) = unsafe { channel.inputStream() } {
                input.close();
            }
            if let Some(output) = unsafe { channel.outputStream() } {
                output.close();
            }
        }
        self.ivars().shared.closed(link);
    }

    /// Stops the radio: no advertising, no scanning, every link closed.
    ///
    /// The offline switch has to reach the antenna. Stopping only the protocol
    /// would leave this node discoverable, which is the one thing the switch
    /// promises it is not.
    fn shut_down(&self) {
        let links: Vec<LinkId> = self.ivars().state.borrow().channels.keys().copied().collect();
        for link in links {
            self.tear_down(link);
        }

        let mut state = self.ivars().state.borrow_mut();
        if let Some(manager) = state.peripheral_manager.take() {
            unsafe { manager.stopAdvertising() };
            if let Some(psm) = state.psm.take() {
                unsafe { manager.unpublishL2CAPChannel(psm) };
            }
        }
        if let Some(central) = state.central.take() {
            unsafe { central.stopScan() };
        }
        state.peripherals.clear();
        state.advertising = false;
        tracing::info!("radio stopped");
    }
}

/// Whether two references name the same peer.
///
/// Compared by identifier rather than by pointer: CoreBluetooth may hand back
/// a different `CBPeripheral` instance for a device already known, and a
/// pointer comparison then connects to it twice.
fn same_peer(left: &CBPeripheral, right: &CBPeripheral) -> bool {
    let left = unsafe { left.identifier() };
    let right = unsafe { right.identifier() };
    left == right
}

/// Whether a stream has reached a state it cannot come back from.
fn is_finished(stream: &NSInputStream) -> bool {
    let status = stream.streamStatus();
    matches!(status, NSStreamStatus::Closed | NSStreamStatus::Error)
}

/// What to tell the user about a radio that will not run.
///
/// Each of these is a different problem with a different fix, and collapsing
/// them into "unavailable" is how a user ends up toggling Bluetooth to solve a
/// permission prompt they never saw.
fn describe_state(state: CBManagerState) -> &'static str {
    match state {
        CBManagerState::PoweredOff => "Bluetooth is switched off",
        CBManagerState::Unauthorized => "this app is not permitted to use Bluetooth",
        CBManagerState::Unsupported => "this device has no Bluetooth LE",
        CBManagerState::Resetting => "the Bluetooth stack is restarting",
        _ => "Bluetooth is not ready",
    }
}

/// A running radio.
///
/// Dropping it does **not** stop the radio — call [`Radio::stop`], or set
/// [`Shared::stop`], which the pump observes. Tying teardown to `Drop` would
/// mean an Objective-C teardown on whatever thread happened to drop it, which
/// is exactly the threading argument this crate is built to avoid.
pub struct Radio {
    queue: DispatchRetained<DispatchQueue>,
    shared: Arc<Shared>,
}

impl Radio {
    /// Starts advertising and scanning.
    ///
    /// Returns as soon as the queue is running; the radio comes up
    /// asynchronously, and a failure arrives as
    /// [`crate::Event::Unavailable`] rather than as an error here — the state
    /// is not known until CoreBluetooth reports it.
    ///
    /// # Errors
    ///
    /// Currently never, on Apple platforms. The result is kept so a caller
    /// written against it keeps working on platforms where
    /// [`RadioError::Unsupported`] is returned instead.
    pub fn start(config: &Config, shared: Arc<Shared>) -> Result<Self, RadioError> {
        let queue = DispatchQueue::new("com.cabalmesh.ble", DispatchQueueAttr::SERIAL);

        let config = config.clone();
        let on_queue = shared.clone();
        let pump_queue = queue.clone();
        let interval = config.pump_interval;

        queue.exec_async(move || {
            let delegate = Delegate::alloc().set_ivars(Ivars {
                shared: on_queue.clone(),
                config: config.clone(),
                state: RefCell::new(State {
                    central: None,
                    peripheral_manager: None,
                    psm: None,
                    peripherals: Vec::new(),
                    channels: HashMap::new(),
                    advertising: false,
                }),
            });
            let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };

            // CoreBluetooth holds its delegate **weakly**. Nothing else here
            // can own it — it may not leave this queue, and the queue has no
            // storage — so it is leaked on purpose. One object, for the life
            // of the process. A dropped delegate is a radio that starts, logs
            // nothing, and never calls back.
            let leaked = Retained::into_raw(delegate);
            // SAFETY: just created above, so not null.
            let delegate: &Delegate = unsafe { &*leaked };

            let protocol = ProtocolObject::from_ref(delegate);
            let central = unsafe {
                CBCentralManager::initWithDelegate_queue(
                    CBCentralManager::alloc(),
                    Some(protocol),
                    Some(&pump_queue),
                )
            };
            let protocol = ProtocolObject::from_ref(delegate);
            let manager = unsafe {
                CBPeripheralManager::initWithDelegate_queue(
                    CBPeripheralManager::alloc(),
                    Some(protocol),
                    Some(&pump_queue),
                )
            };

            {
                let mut state = delegate.ivars().state.borrow_mut();
                state.central = Some(central);
                state.peripheral_manager = Some(manager);
            }

            schedule_pump(pump_queue.clone(), QueueOnly(leaked), on_queue, interval);
        });

        Ok(Self { queue, shared })
    }

    /// Stops advertising, scanning and every link.
    pub fn stop(&self) {
        self.shared.stop();
        // The pump sees the flag on its next tick and tears the radio down on
        // the queue that owns it. Nothing Objective-C happens on this thread.
        let _ = &self.queue;
    }
}

/// A delegate pointer that may be moved between threads but only *used* on the
/// queue that owns it.
///
/// `dispatch2` requires the work it schedules to be `Send`; the delegate holds
/// a `RefCell` and is emphatically not `Sync`. The two are reconciled by the
/// invariant this whole file is built on — every dereference happens on the one
/// serial queue — rather than by making the delegate thread-safe, which would
/// mean locking state that is never contended.
struct QueueOnly(*const Delegate);

// SAFETY: the pointer is dereferenced only inside blocks executed on the
// serial queue the delegate was created on, and the delegate is leaked, so it
// outlives every block that can observe it.
unsafe impl Send for QueueOnly {}

/// Re-arms the pump until the radio is stopped.
fn schedule_pump(
    queue: DispatchRetained<DispatchQueue>,
    delegate: QueueOnly,
    shared: Arc<Shared>,
    interval: std::time::Duration,
) {
    let next = queue.clone();
    let result = queue.after(DispatchTime::try_from(interval).unwrap_or(DispatchTime::NOW), move || {
        // Captured whole rather than by field: Rust 2021 closures capture
        // `delegate.0` alone otherwise, and a bare `*const Delegate` is not
        // `Send` however safe the wrapper around it is.
        let delegate = delegate;
        // SAFETY: this block runs on the queue that owns the delegate, and the
        // delegate is leaked rather than dropped.
        let this = unsafe { &*delegate.0 };
        if shared.is_stopped() {
            this.shut_down();
            return;
        }
        this.pump();
        schedule_pump(next.clone(), QueueOnly(delegate.0), shared.clone(), interval);
    });

    if result.is_err() {
        tracing::error!("could not schedule the BLE pump; the radio will not move bytes");
    }
}
