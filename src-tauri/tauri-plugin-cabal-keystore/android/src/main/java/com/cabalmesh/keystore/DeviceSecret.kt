package com.cabalmesh.keystore

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The device half of the vault key.
 *
 * ## What is actually protected, and by what
 *
 * The 32 random bytes handed back are **not** themselves in the Keystore —
 * Android's Keystore stores keys, not arbitrary secrets. What lives there is
 * an AES key that cannot be exported, in StrongBox where the device has it and
 * in the TEE otherwise. The secret is encrypted under that key and the
 * resulting blob is kept in ordinary app storage.
 *
 * That arrangement is what makes a copied file useless: the blob travels, the
 * Keystore key does not and cannot. Reading the app's data directory off a
 * rooted phone or a backup yields ciphertext whose key is in hardware on the
 * device it was made on.
 *
 * ## What it is not
 *
 * `setUserAuthenticationRequired` is deliberately **false**. With it, every
 * vault read would demand a fingerprint, and the unlock passphrase already
 * covers "someone else is holding this phone". Without it, this buys
 * exfiltration resistance rather than access control: code already running as
 * this app, on this device, can ask the Keystore to decrypt. That trade is
 * recorded here and in `device_binding.rs` rather than being discovered later
 * from the absence of a prompt.
 */
internal object DeviceSecret {
    private const val KEY_ALIAS = "cabalmesh-vault-device-key"
    private const val KEYSTORE = "AndroidKeyStore"
    private const val PREFERENCES = "cabalmesh-keystore"
    private const val BLOB = "device-secret-blob"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val SECRET_BYTES = 32
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128

    /** Whether the Keystore key ended up in StrongBox, for honest reporting. */
    @Volatile
    var strongBoxBacked: Boolean = false
        private set

    /**
     * The secret for this install, creating it on first use.
     *
     * Synchronized, and re-reading inside the lock: two callers that both found
     * nothing would otherwise both generate, and the second would overwrite the
     * first — leaving a vault wrapped under a secret that no longer exists.
     */
    @Synchronized
    fun get(context: Context): ByteArray {
        val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        val key = loadOrCreateKey()

        preferences.getString(BLOB, null)?.let { stored ->
            return decrypt(key, Base64.decode(stored, Base64.NO_WRAP))
        }

        val fresh = ByteArray(SECRET_BYTES).also { java.security.SecureRandom().nextBytes(it) }
        preferences
            .edit()
            .putString(BLOB, Base64.encodeToString(encrypt(key, fresh), Base64.NO_WRAP))
            .commit()
        return fresh
    }

    private fun loadOrCreateKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let {
            // An existing key's backing is not re-queried here; it was recorded
            // when it was created and the blob is bound to it either way.
            return it.secretKey
        }

        // StrongBox first, falling back to the TEE. A device without a secure
        // element still gets a non-exportable key, which is the property that
        // matters; refusing to run on such devices would trade a real
        // improvement for nothing.
        return try {
            generate(strongBox = true).also { strongBoxBacked = true }
        } catch (_: StrongBoxUnavailableException) {
            generate(strongBox = false).also { strongBoxBacked = false }
        }
    }

    private fun generate(strongBox: Boolean): SecretKey {
        val builder = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            // See the class comment: the passphrase is the access control.
            .setUserAuthenticationRequired(false)

        if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            builder.setIsStrongBoxBacked(true)
        }

        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
            .apply { init(builder.build()) }
            .generateKey()
    }

    private fun encrypt(key: SecretKey, plaintext: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply { init(Cipher.ENCRYPT_MODE, key) }
        // The IV is generated by the Keystore, never chosen here: reusing one
        // under a single GCM key breaks it completely.
        return cipher.iv + cipher.doFinal(plaintext)
    }

    private fun decrypt(key: SecretKey, blob: ByteArray): ByteArray {
        val iv = blob.copyOfRange(0, IV_BYTES)
        val ciphertext = blob.copyOfRange(IV_BYTES, blob.size)
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, iv))
        }
        return cipher.doFinal(ciphertext)
    }
}
