package com.cabalmesh.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.os.ParcelUuid
import android.util.Log
import java.io.IOException
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * The Android BLE radio.
 *
 * Advertises, scans, runs the two-stage rendezvous, and moves bytes over
 * L2CAP. It contains no routing, no framing and no identity — those live in
 * `cabal-ble`, which has no I/O and is tested on the host. A bug in routing
 * must be fixable with a test rather than with two phones.
 *
 * ## The rendezvous, in two stages
 *
 * A BLE advertisement has room for a service UUID and little else, so the PSM
 * an L2CAP channel needs cannot be advertised directly.
 *
 * 1. **GATT, once.** Open an L2CAP server socket, learn its PSM, expose that
 *    PSM as a read-only characteristic, advertise the service. A central scans,
 *    connects, reads two bytes, and is done with GATT.
 * 2. **L2CAP, for everything after.** A reliable, ordered stream — which is
 *    why there is no fragmentation layer anywhere in this design.
 *
 * ## Threads
 *
 * Android hands GATT callbacks to binder threads. Sockets block. So: one
 * thread accepting inbound channels, one connecting each outbound channel, and
 * a reader and a writer per link. State that more than one of them touches is
 * in concurrent collections.
 */
@SuppressLint("MissingPermission")
class BleRadio(
    private val context: Context,
    private val serviceUuid: UUID,
    private val psmUuid: UUID,
    private val onEvent: (RadioEvent) -> Unit,
) {
    /** Something the radio observed, mirroring the Rust enum it deserialises into. */
    sealed class RadioEvent {
        data class Up(val link: Long) : RadioEvent()
        data class Down(val link: Long) : RadioEvent()
        data class Bytes(val link: Long, val data: ByteArray) : RadioEvent()
        data class Unavailable(val reason: String) : RadioEvent()
    }

    private val running = AtomicBoolean(false)
    private val nextLink = AtomicLong(1)

    private val links = ConcurrentHashMap<Long, Link>()

    /**
     * Devices we must not start another connection to, by address.
     *
     * A scan reports the same device several times a second. Without this each
     * report starts another GATT connection, and a peer in range becomes a
     * connection storm rather than one link.
     *
     * An address leaves this set in exactly two places: when the GATT exchange
     * fails before a channel was attempted, and when a link goes down. It is
     * emphatically *not* removed when GATT disconnects, because disconnecting
     * from GATT is the **normal** end of the rendezvous — the PSM has been read
     * and everything after it is L2CAP. Removing it there made every scan
     * result open another channel: the first run against two emulators climbed
     * to nine links between two nodes in under a second.
     */
    private val known = ConcurrentHashMap<String, Boolean>()

    /**
     * Which address each link belongs to, so a link going down releases its
     * device for reconnection and nothing else does.
     */
    private val linkAddress = ConcurrentHashMap<Long, String>()

    private var adapter: BluetoothAdapter? = null
    private var gattServer: BluetoothGattServer? = null
    private var serverSocket: BluetoothServerSocket? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var scanCallback: ScanCallback? = null
    private var accepting: Thread? = null

    private class Link(
        val socket: BluetoothSocket,
        val outbox: LinkedBlockingQueue<ByteArray>,
        val reader: Thread,
        val writer: Thread,
    )

    /**
     * Brings the radio up.
     *
     * @return null on success, or what went wrong — a refused permission and a
     * powered-off adapter are different problems and the caller has to be able
     * to say which.
     */
    fun start(): String? {
        if (running.getAndSet(true)) return null

        val manager = context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
            ?: return "this device has no Bluetooth service"
        val adapter = manager.adapter ?: return "this device has no Bluetooth adapter"
        this.adapter = adapter

        if (!adapter.isEnabled) return "Bluetooth is switched off"

        val psm = try {
            // Insecure: pairing is not wanted. Authentication is the Noise
            // handshake's job, one layer up, and requiring a pairing dialog
            // would put a tap between two strangers and a mesh.
            val socket = adapter.listenUsingInsecureL2capChannel()
            serverSocket = socket
            socket.psm
        } catch (error: Throwable) {
            return "could not open an L2CAP channel: ${error.message}"
        }
        Log.i(TAG, "published an L2CAP channel psm=$psm")

        try {
            startGattServer(manager, psm)
            startAdvertising(adapter)
            startScanning(adapter)
        } catch (error: Throwable) {
            return "could not start the radio: ${error.message}"
        }

        accepting = Thread({ acceptLoop() }, "cabal-ble-accept").also {
            it.isDaemon = true
            it.start()
        }
        return null
    }

    private fun startGattServer(manager: BluetoothManager, psm: Int) {
        val characteristic = BluetoothGattCharacteristic(
            psmUuid,
            BluetoothGattCharacteristic.PROPERTY_READ,
            BluetoothGattCharacteristic.PERMISSION_READ,
        )
        // Big-endian, two bytes, matching what the reading side parses. A
        // disagreement here opens a channel to whatever else is listening.
        characteristic.value = byteArrayOf((psm shr 8).toByte(), psm.toByte())

        val service = BluetoothGattService(serviceUuid, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        service.addCharacteristic(characteristic)

        val server = manager.openGattServer(context, object : BluetoothGattServerCallback() {
            override fun onCharacteristicReadRequest(
                device: BluetoothDevice,
                requestId: Int,
                offset: Int,
                requested: BluetoothGattCharacteristic,
            ) {
                val value = requested.value ?: ByteArray(0)
                // Offsets are honoured even though the value is two bytes: a
                // central is allowed to ask for a slice, and answering the
                // whole value to an offset read corrupts what it assembles.
                val slice = if (offset >= value.size) ByteArray(0) else value.copyOfRange(offset, value.size)
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, slice)
            }
        })
        server.addService(service)
        gattServer = server
    }

    private fun startAdvertising(adapter: BluetoothAdapter) {
        val advertiser = adapter.bluetoothLeAdvertiser
            ?: throw IllegalStateException("this device cannot advertise")

        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_BALANCED)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(true)
            .setTimeout(0)
            .build()

        // The service UUID and nothing else. No device name: it is a durable
        // identifier broadcast in the clear, which is the exact thing the
        // ephemeral peer id exists to avoid.
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceUuid(ParcelUuid(serviceUuid))
            .build()

        val callback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
                Log.i(TAG, "advertising")
            }

            override fun onStartFailure(errorCode: Int) {
                onEvent(RadioEvent.Unavailable("advertising failed with code $errorCode"))
            }
        }
        advertiser.startAdvertising(settings, data, callback)
        advertiseCallback = callback
    }

    private fun startScanning(adapter: BluetoothAdapter) {
        val scanner = adapter.bluetoothLeScanner
            ?: throw IllegalStateException("this device cannot scan")

        val filters = listOf(
            ScanFilter.Builder().setServiceUuid(ParcelUuid(serviceUuid)).build()
        )
        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_BALANCED)
            .build()

        val callback = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                connect(result.device)
            }

            override fun onScanFailed(errorCode: Int) {
                onEvent(RadioEvent.Unavailable("scanning failed with code $errorCode"))
            }
        }
        scanner.startScan(filters, settings, callback)
        scanCallback = callback
        Log.i(TAG, "scanning for nodes")
    }

    /** Connects to a discovered peer, once. */
    private fun connect(device: BluetoothDevice) {
        if (!running.get()) return
        if (known.putIfAbsent(device.address, true) != null) return

        Log.d(TAG, "discovered a node; connecting")
        device.connectGatt(context, false, object : BluetoothGattCallback() {
            override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
                if (newState == BluetoothAdapter.STATE_CONNECTED) {
                    gatt.discoverServices()
                } else if (newState == BluetoothAdapter.STATE_DISCONNECTED) {
                    // Deliberately does not release the address: this fires on
                    // the ordinary end of the rendezvous, and releasing here is
                    // what turned one peer into nine links.
                    gatt.close()
                }
            }

            override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
                if (status != BluetoothGatt.GATT_SUCCESS) return
                val characteristic = gatt.getService(serviceUuid)?.getCharacteristic(psmUuid)
                if (characteristic == null) {
                    // Not one of ours after all. Released, so a device that
                    // later starts advertising the service can be reached.
                    known.remove(device.address)
                    gatt.disconnect()
                    return
                }
                gatt.readCharacteristic(characteristic)
            }

            @Suppress("DEPRECATION")
            override fun onCharacteristicRead(
                gatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                status: Int,
            ) {
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    known.remove(device.address)
                    return
                }
                val value = characteristic.value
                if (value == null || value.size < 2) {
                    Log.d(TAG, "the PSM characteristic was not two bytes")
                    known.remove(device.address)
                    return
                }
                val psm = ((value[0].toInt() and 0xFF) shl 8) or (value[1].toInt() and 0xFF)
                // GATT has done its job. The connection is dropped rather than
                // held: everything after this is L2CAP, and a GATT link left
                // open is a connection slot spent on nothing.
                gatt.disconnect()
                openChannel(device, psm)
            }
        })
    }

    /** Opens an L2CAP channel to a peer whose PSM is known. */
    private fun openChannel(device: BluetoothDevice, psm: Int) {
        Thread({
            try {
                val socket = device.createInsecureL2capChannel(psm)
                socket.connect()
                adopt(socket, device.address)
            } catch (error: IOException) {
                Log.d(TAG, "L2CAP connect failed: ${error.message}")
                known.remove(device.address)
            }
        }, "cabal-ble-connect").apply {
            isDaemon = true
            start()
        }
    }

    /** Accepts inbound channels for as long as the radio runs. */
    private fun acceptLoop() {
        while (running.get()) {
            val socket = try {
                serverSocket?.accept() ?: return
            } catch (error: IOException) {
                // Closing the server socket is how `stop` unblocks this, so an
                // exception after a stop is the expected exit rather than a
                // failure worth reporting.
                if (running.get()) {
                    Log.d(TAG, "accept failed: ${error.message}")
                }
                return
            }
            // Inbound: the peer's address is on the socket's remote device.
            adopt(socket, socket.remoteDevice?.address)
        }
    }

    /** Takes ownership of a connected socket, from either role. */
    private fun adopt(socket: BluetoothSocket, address: String?) {
        val link = nextLink.getAndIncrement()
        val outbox = LinkedBlockingQueue<ByteArray>()

        val reader = Thread({
            val buffer = ByteArray(READ_CHUNK)
            try {
                while (running.get()) {
                    val count = socket.inputStream.read(buffer)
                    if (count <= 0) break
                    onEvent(RadioEvent.Bytes(link, buffer.copyOf(count)))
                }
            } catch (_: IOException) {
                // The peer walked away. Ordinary.
            } finally {
                tearDown(link)
            }
        }, "cabal-ble-read-$link")

        val writer = Thread({
            try {
                while (running.get()) {
                    val chunk = outbox.take()
                    if (chunk.isEmpty()) break
                    socket.outputStream.write(chunk)
                    socket.outputStream.flush()
                }
            } catch (_: InterruptedException) {
            } catch (_: IOException) {
            } finally {
                tearDown(link)
            }
        }, "cabal-ble-write-$link")

        links[link] = Link(socket, outbox, reader, writer)
        address?.let {
            linkAddress[link] = it
            // Inbound links claim the address too, so the two nodes do not each
            // dial the other after already being connected.
            known[it] = true
        }
        reader.isDaemon = true
        writer.isDaemon = true
        reader.start()
        writer.start()

        Log.i(TAG, "link up $link")
        onEvent(RadioEvent.Up(link))
    }

    /** Queues bytes for a link. Returns whether the link is still open. */
    fun send(link: Long, data: ByteArray): Boolean {
        val known = links[link] ?: return false
        return known.outbox.offer(data)
    }

    /** Tears a link down, once, however many threads noticed it failing. */
    fun tearDown(link: Long) {
        val removed = links.remove(link) ?: return
        linkAddress.remove(link)?.let { known.remove(it) }
        try {
            removed.socket.close()
        } catch (_: IOException) {
        }
        // An empty chunk is the writer's signal to stop; without it the writer
        // blocks on `take()` forever and the thread leaks per link.
        removed.outbox.offer(ByteArray(0))
        Log.i(TAG, "link down $link")
        onEvent(RadioEvent.Down(link))
    }

    /**
     * Stops everything: advertising, scanning, every link, the server socket.
     *
     * The offline switch has to reach the antenna. Stopping only the protocol
     * would leave this node discoverable, which is the one thing the switch
     * promises it is not.
     */
    fun stop() {
        if (!running.getAndSet(false)) return

        links.keys.toList().forEach { tearDown(it) }

        try {
            advertiseCallback?.let { adapter?.bluetoothLeAdvertiser?.stopAdvertising(it) }
            scanCallback?.let { adapter?.bluetoothLeScanner?.stopScan(it) }
            gattServer?.close()
            serverSocket?.close()
        } catch (error: Throwable) {
            Log.d(TAG, "stop: ${error.message}")
        }

        advertiseCallback = null
        scanCallback = null
        gattServer = null
        serverSocket = null
        known.clear()
        Log.i(TAG, "radio stopped")
    }

    companion object {
        const val TAG = "CabalBle"

        /**
         * Bytes read per call.
         *
         * One L2CAP SDU is up to 64 KiB; this is read in a loop, so it is a
         * buffer choice rather than a limit.
         */
        const val READ_CHUNK = 4096
    }
}
