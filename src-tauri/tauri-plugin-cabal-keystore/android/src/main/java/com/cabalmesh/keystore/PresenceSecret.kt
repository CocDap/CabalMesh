package com.cabalmesh.keystore

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.hardware.biometrics.BiometricManager.Authenticators.DEVICE_CREDENTIAL
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The secret behind the PIN path: released only after the person enters the
 * **device** passcode/pattern, whose attempts the OS throttles in hardware.
 * See `src/presence.rs` for why this — not a counter on the app's own PIN,
 * which no Android API offers — is what makes a 6-digit PIN defensible.
 *
 * Separate from [DeviceSecret] on purpose: that key is deliberately usable
 * without the user, this one deliberately is not. Device credential only,
 * never biometrics (`setUserAuthenticationParameters(.., AUTH_DEVICE_CREDENTIAL)`).
 *
 * Requires API 30 for device-credential-only key auth. Below that,
 * [isSupported] is false and the app offers a passphrase instead.
 *
 * NOT VERIFIED ON A DEVICE in the change that added it.
 */
internal object PresenceSecret {
    private const val KEY_ALIAS = "cabalmesh-presence-key"
    private const val KEYSTORE = "AndroidKeyStore"
    private const val PREFERENCES = "cabalmesh-keystore"
    private const val BLOB = "presence-secret-blob"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128
    /** Seconds the key stays usable after the prompt: enough for one use. */
    private const val AUTH_WINDOW_SECONDS = 10

    fun isSupported(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return false
        val keyguard = context.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager
        return keyguard.isDeviceSecure
    }

    /** Shows the device-credential prompt; on success hands back the secret. */
    fun request(activity: Activity, onSecret: (ByteArray) -> Unit, onError: (String) -> Unit) {
        if (!isSupported(activity)) {
            onError("unsupported")
            return
        }
        activity.runOnUiThread {
            val prompt = BiometricPrompt.Builder(activity)
                .setTitle("Unlock CabalMesh")
                .setDescription("Confirm your screen lock to open the vault")
                .setAllowedAuthenticators(DEVICE_CREDENTIAL)
                .build()
            prompt.authenticate(
                CancellationSignal(),
                activity.mainExecutor,
                object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        try {
                            onSecret(get(activity.applicationContext))
                        } catch (error: Throwable) {
                            onError(error.message ?: "the Android Keystore refused")
                        }
                    }

                    override fun onAuthenticationError(code: Int, message: CharSequence) {
                        onError(message.toString())
                    }
                },
            )
        }
    }

    @Synchronized
    private fun get(context: Context): ByteArray {
        val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        val key = loadOrCreateKey()
        preferences.getString(BLOB, null)?.let { stored ->
            return decrypt(key, Base64.decode(stored, Base64.NO_WRAP))
        }
        val fresh = ByteArray(32).also { java.security.SecureRandom().nextBytes(it) }
        preferences.edit().putString(BLOB, Base64.encodeToString(encrypt(key, fresh), Base64.NO_WRAP)).commit()
        return fresh
    }

    private fun loadOrCreateKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setUserAuthenticationParameters(AUTH_WINDOW_SECONDS, KeyProperties.AUTH_DEVICE_CREDENTIAL)
            // Removing the screen lock invalidates the key: the vault then
            // needs a guardian restore or an exported key, never a silent
            // downgrade to no presence check.
            .build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
            .apply { init(spec) }
            .generateKey()
    }

    private fun encrypt(key: SecretKey, plaintext: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply { init(Cipher.ENCRYPT_MODE, key) }
        return cipher.iv + cipher.doFinal(plaintext)
    }

    private fun decrypt(key: SecretKey, blob: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, blob.copyOfRange(0, IV_BYTES)))
        }
        return cipher.doFinal(blob.copyOfRange(IV_BYTES, blob.size))
    }
}
