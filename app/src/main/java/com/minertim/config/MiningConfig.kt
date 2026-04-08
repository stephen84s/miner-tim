package com.minertim.config

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import com.minertim.security.SecurityValidator
import com.minertim.security.ValidationResult
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import android.util.Base64
import java.security.SecureRandom

class MiningConfig(private val context: Context) {
    companion object {
        private const val TAG = "MiningConfig"
        private const val ENCRYPTED_PREFS_NAME = "mining_config_secure"
        private const val KEY_ALIAS = "mining_key"
        const val KEY_POOL_ADDRESS = "pool_address"
        const val KEY_WALLET_ADDRESS = "wallet_address"
        const val KEY_THREAD_COUNT = "thread_count"
        const val KEY_MAX_CPU_TEMP = "max_cpu_temp"
        const val KEY_MIN_BATTERY_LEVEL = "min_battery_level"
        const val KEY_MINING_INTENSITY = "mining_intensity"
        const val KEY_AUTO_START = "auto_start"
        const val KEY_WIFI_ONLY = "wifi_only"

        // AES/GCM parameters
        private const val GCM_IV_LENGTH = 12   // 96-bit IV recommended for GCM
        private const val GCM_TAG_BITS = 128   // 128-bit authentication tag

        // Default values
        const val DEFAULT_POOL_ADDRESS = "pool.supportxmr.com:443"
        const val DEFAULT_THREAD_COUNT = 2
        const val DEFAULT_MAX_CPU_TEMP = 75.0f
        const val DEFAULT_MIN_BATTERY_LEVEL = 20
        const val DEFAULT_MINING_INTENSITY = 50
    }

    private val prefs: SharedPreferences = context.getSharedPreferences(ENCRYPTED_PREFS_NAME, Context.MODE_PRIVATE)
    private val encryptionKey: SecretKey by lazy { getOrCreateKey() }

    fun getPoolAddress(): String {
        return prefs.getString(KEY_POOL_ADDRESS, DEFAULT_POOL_ADDRESS) ?: DEFAULT_POOL_ADDRESS
    }

    fun setPoolAddress(address: String): Boolean {
        // Validate pool address before storing
        val validation = SecurityValidator.validatePoolAddress(address)
        if (!validation.isValid) {
            Log.e(TAG, "Invalid pool address: ${validation.errorMessage}")
            return false
        }

        val sanitized = SecurityValidator.sanitizeInput(address)
        prefs.edit().putString(KEY_POOL_ADDRESS, sanitized).apply()
        Log.d(TAG, "Pool address stored successfully")

        if (validation.warningMessage != null) {
            Log.w(TAG, "Pool address warning: ${validation.warningMessage}")
        }

        return true
    }

    fun getWalletAddress(): String {
        val encrypted = prefs.getString(KEY_WALLET_ADDRESS, "") ?: ""
        return if (encrypted.isNotEmpty()) {
            try {
                decrypt(encrypted)
            } catch (e: Exception) {
                Log.e(TAG, "Failed to decrypt wallet address", e)
                ""
            }
        } else {
            ""
        }
    }

    fun setWalletAddress(address: String): Boolean {
        // Validate address before storing
        val validation = SecurityValidator.validateMoneroAddress(address)
        if (!validation.isValid) {
            Log.e(TAG, "Invalid wallet address: ${validation.errorMessage}")
            return false
        }

        try {
            val encrypted = encrypt(address)
            prefs.edit().putString(KEY_WALLET_ADDRESS, encrypted).apply()
            Log.d(TAG, "Wallet address stored successfully")
            return true
        } catch (e: Exception) {
            Log.e(TAG, "Failed to encrypt wallet address", e)
            return false
        }
    }

    fun getThreadCount(): Int {
        val maxThreads = Runtime.getRuntime().availableProcessors()
        val savedThreads = prefs.getInt(KEY_THREAD_COUNT, DEFAULT_THREAD_COUNT)
        return minOf(savedThreads, maxThreads)
    }

    fun setThreadCount(count: Int): Boolean {
        val maxThreads = Runtime.getRuntime().availableProcessors()

        if (count <= 0 || count > maxThreads * 2) {
            Log.e(TAG, "Invalid thread count: $count")
            return false
        }

        val validCount = minOf(maxOf(1, count), maxThreads)
        prefs.edit().putInt(KEY_THREAD_COUNT, validCount).apply()

        if (validCount != count) {
            Log.i(TAG, "Thread count adjusted from $count to $validCount")
        }

        return true
    }

    fun getMaxCpuTemp(): Float {
        return prefs.getFloat(KEY_MAX_CPU_TEMP, DEFAULT_MAX_CPU_TEMP)
    }

    fun setMaxCpuTemp(temp: Float): Boolean {
        if (temp < 40.0f || temp > 90.0f) {
            Log.e(TAG, "Invalid CPU temperature: $temp")
            return false
        }

        prefs.edit().putFloat(KEY_MAX_CPU_TEMP, temp).apply()

        if (temp > 80.0f) {
            Log.w(TAG, "High CPU temperature limit set: ${temp}°C")
        }

        return true
    }

    fun getMinBatteryLevel(): Int {
        return prefs.getInt(KEY_MIN_BATTERY_LEVEL, DEFAULT_MIN_BATTERY_LEVEL)
    }

    fun setMinBatteryLevel(level: Int): Boolean {
        if (level < 5 || level > 95) {
            Log.e(TAG, "Invalid battery level: $level")
            return false
        }

        prefs.edit().putInt(KEY_MIN_BATTERY_LEVEL, level).apply()

        if (level < 15) {
            Log.w(TAG, "Low minimum battery level set: ${level}%")
        }

        return true
    }

    fun getMiningIntensity(): Int {
        return prefs.getInt(KEY_MINING_INTENSITY, DEFAULT_MINING_INTENSITY)
    }

    fun setMiningIntensity(intensity: Int): Boolean {
        if (intensity < 1 || intensity > 100) {
            Log.e(TAG, "Invalid mining intensity: $intensity")
            return false
        }

        prefs.edit().putInt(KEY_MINING_INTENSITY, intensity).apply()
        return true
    }

    fun getAutoStart(): Boolean {
        return prefs.getBoolean(KEY_AUTO_START, false)
    }

    fun setAutoStart(enabled: Boolean) {
        prefs.edit().putBoolean(KEY_AUTO_START, enabled).apply()
    }

    fun getWifiOnly(): Boolean {
        return prefs.getBoolean(KEY_WIFI_ONLY, true)
    }

    fun setWifiOnly(enabled: Boolean) {
        prefs.edit().putBoolean(KEY_WIFI_ONLY, enabled).apply()
    }

    fun isConfigurationValid(): Boolean {
        val walletAddress = getWalletAddress()
        val poolAddress = getPoolAddress()

        if (walletAddress.isEmpty() || poolAddress.isEmpty()) {
            return false
        }

        // Validate both addresses
        val walletValid = SecurityValidator.validateMoneroAddress(walletAddress).isValid
        val poolValid = SecurityValidator.validatePoolAddress(poolAddress).isValid

        return walletValid && poolValid
    }

    fun validateAllSettings(): ValidationResult {
        return SecurityValidator.validateMiningConfig(
            getThreadCount(),
            getMaxCpuTemp(),
            getMinBatteryLevel(),
            getMiningIntensity()
        )
    }

    fun getConfigHash(): String {
        return SecurityValidator.generateConfigHash(
            getPoolAddress(),
            getWalletAddress()
        )
    }

    private fun getOrCreateKey(): SecretKey {
        val keyString = prefs.getString(KEY_ALIAS, null)

        return if (keyString != null) {
            try {
                val keyBytes = Base64.decode(keyString, Base64.DEFAULT)
                SecretKeySpec(keyBytes, "AES")
            } catch (e: Exception) {
                Log.e(TAG, "Failed to restore key, generating new one", e)
                generateAndStoreKey()
            }
        } else {
            generateAndStoreKey()
        }
    }

    private fun generateAndStoreKey(): SecretKey {
        val keyGenerator = KeyGenerator.getInstance("AES")
        keyGenerator.init(256)
        val key = keyGenerator.generateKey()

        val keyString = Base64.encodeToString(key.encoded, Base64.DEFAULT)
        prefs.edit().putString(KEY_ALIAS, keyString).apply()

        return key
    }

    private fun encrypt(plaintext: String): String {
        val iv = ByteArray(GCM_IV_LENGTH)
        SecureRandom().nextBytes(iv)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, encryptionKey, GCMParameterSpec(GCM_TAG_BITS, iv))
        val ciphertext = cipher.doFinal(plaintext.toByteArray(Charsets.UTF_8))
        // Prepend IV to ciphertext before Base64-encoding
        val combined = iv + ciphertext
        return Base64.encodeToString(combined, Base64.DEFAULT)
    }

    private fun decrypt(encoded: String): String {
        val combined = Base64.decode(encoded, Base64.DEFAULT)
        if (combined.size > GCM_IV_LENGTH) {
            try {
                // GCM format: first GCM_IV_LENGTH bytes are the IV
                val iv = combined.copyOfRange(0, GCM_IV_LENGTH)
                val ciphertext = combined.copyOfRange(GCM_IV_LENGTH, combined.size)
                val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                cipher.init(Cipher.DECRYPT_MODE, encryptionKey, GCMParameterSpec(GCM_TAG_BITS, iv))
                return String(cipher.doFinal(ciphertext), Charsets.UTF_8)
            } catch (_: Exception) {
                // GCM auth failed — fall through to legacy ECB path
            }
        }
        // Legacy ECB fallback — re-encrypt with GCM on next save
        val cipher = Cipher.getInstance("AES/ECB/PKCS5Padding")
        cipher.init(Cipher.DECRYPT_MODE, encryptionKey)
        return String(cipher.doFinal(combined), Charsets.UTF_8)
    }

    fun getRecommendedThreadCount(): Int {
        val cores = Runtime.getRuntime().availableProcessors()
        return when {
            cores <= 2 -> 1
            cores <= 4 -> 2
            cores <= 8 -> cores - 2
            else -> cores / 2
        }
    }
}
