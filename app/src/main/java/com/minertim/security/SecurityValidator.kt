package com.minertim.security

import java.security.MessageDigest

enum class RiskLevel {
    LOW,
    MEDIUM,
    HIGH,
    CRITICAL
}

data class ValidationResult(
    val isValid: Boolean,
    val errorMessage: String? = null,
    val warningMessage: String? = null,
    val riskLevel: RiskLevel = RiskLevel.LOW
)

object SecurityValidator {

    private const val TAG = "SecurityValidator"

    private val WHITELISTED_POOLS = setOf(
        "supportxmr.com",
        "xmrpool.eu",
        "nanopool.org",
        "hashvault.pro",
        "monerohash.com",
        "xmrpool.net",
        "minexmr.com",
        "herominers.com",
        "c3pool.com",
        "moneroocean.stream",
        "miningpoolhub.com",
        "2miners.com"
    )

    // Mainnet: starts with 4, 95 chars
    // Integrated: starts with 4, 106 chars
    // Testnet: starts with 9, A, or B, 95 chars
    // Stagenet: starts with 5, 95 chars
    private val MAINNET_REGEX = Regex("^4[1-9A-HJ-NP-Za-km-z]{94}$")
    private val INTEGRATED_REGEX = Regex("^4[1-9A-HJ-NP-Za-km-z]{105}$")
    private val TESTNET_REGEX = Regex("^[9AB][1-9A-HJ-NP-Za-km-z]{94}$")
    private val STAGENET_REGEX = Regex("^5[1-9A-HJ-NP-Za-km-z]{94}$")

    private val POOL_ADDRESS_REGEX = Regex("^[a-zA-Z0-9][a-zA-Z0-9.\\-]*:\\d{1,5}$")

    private val DANGEROUS_PATTERNS = listOf(
        "<script",
        "javascript:",
        "eval(",
        "..",
        "file://",
        "../",
        "..\\",
        "%2e%2e",
        "%2f"
    )

    fun validateMoneroAddress(address: String?): ValidationResult = validateWalletAddress(address)

    fun validateWalletAddress(address: String?): ValidationResult {
        if (address.isNullOrBlank()) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Wallet address is required",
                riskLevel = RiskLevel.CRITICAL
            )
        }

        val sanitized = address.trim()

        if (containsDangerousInput(sanitized)) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Wallet address contains invalid characters",
                riskLevel = RiskLevel.CRITICAL
            )
        }

        return when {
            MAINNET_REGEX.matches(sanitized) -> ValidationResult(
                isValid = true,
                riskLevel = RiskLevel.LOW
            )
            INTEGRATED_REGEX.matches(sanitized) -> ValidationResult(
                isValid = true,
                warningMessage = "Integrated address detected. Ensure the payment ID is correct.",
                riskLevel = RiskLevel.MEDIUM
            )
            TESTNET_REGEX.matches(sanitized) -> ValidationResult(
                isValid = true,
                warningMessage = "Testnet address detected. No real XMR will be mined.",
                riskLevel = RiskLevel.MEDIUM
            )
            STAGENET_REGEX.matches(sanitized) -> ValidationResult(
                isValid = true,
                warningMessage = "Stagenet address detected. No real XMR will be mined.",
                riskLevel = RiskLevel.MEDIUM
            )
            else -> ValidationResult(
                isValid = false,
                errorMessage = "Invalid Monero address format. Expected mainnet (4..., 95 chars), " +
                    "integrated (4..., 106 chars), testnet (9/A/B..., 95 chars), " +
                    "or stagenet (5..., 95 chars)",
                riskLevel = RiskLevel.HIGH
            )
        }
    }

    fun validatePoolAddress(poolAddress: String?): ValidationResult {
        if (poolAddress.isNullOrBlank()) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Pool address is required",
                riskLevel = RiskLevel.CRITICAL
            )
        }

        val sanitized = poolAddress.trim()

        if (containsDangerousInput(sanitized)) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Pool address contains invalid characters",
                riskLevel = RiskLevel.CRITICAL
            )
        }

        if (!POOL_ADDRESS_REGEX.matches(sanitized)) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Invalid pool address format. Expected host:port (e.g., pool.supportxmr.com:443)",
                riskLevel = RiskLevel.HIGH
            )
        }

        val host = sanitized.substringBeforeLast(":")
        val portStr = sanitized.substringAfterLast(":")
        val port = portStr.toIntOrNull()

        if (port == null || port < 1 || port > 65535) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Invalid port number. Must be between 1 and 65535",
                riskLevel = RiskLevel.HIGH
            )
        }

        val isWhitelisted = WHITELISTED_POOLS.any { pool ->
            host.equals(pool, ignoreCase = true) || host.endsWith(".$pool", ignoreCase = true)
        }

        return if (isWhitelisted) {
            ValidationResult(
                isValid = true,
                riskLevel = RiskLevel.LOW
            )
        } else {
            ValidationResult(
                isValid = true,
                warningMessage = "Unknown mining pool. Use at your own risk. " +
                    "Verify the pool is legitimate before mining.",
                riskLevel = RiskLevel.HIGH
            )
        }
    }

    fun validateMiningConfig(
        threads: Int,
        maxTemp: Float,
        minBattery: Int,
        intensity: Int
    ): ValidationResult {
        val maxThreads = Runtime.getRuntime().availableProcessors()

        if (threads < 1 || threads > maxThreads) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Thread count must be between 1 and $maxThreads",
                riskLevel = RiskLevel.HIGH
            )
        }

        if (maxTemp < 40f || maxTemp > 90f) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Max CPU temperature must be between 40 and 90 degrees Celsius",
                riskLevel = RiskLevel.HIGH
            )
        }

        if (minBattery < 5 || minBattery > 95) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Minimum battery level must be between 5% and 95%",
                riskLevel = RiskLevel.HIGH
            )
        }

        if (intensity < 1 || intensity > 100) {
            return ValidationResult(
                isValid = false,
                errorMessage = "Mining intensity must be between 1 and 100",
                riskLevel = RiskLevel.HIGH
            )
        }

        val warnings = mutableListOf<String>()

        if (maxTemp > 80f) {
            warnings.add("High temperature threshold may damage your device.")
        }
        if (minBattery < 10) {
            warnings.add("Very low battery threshold may cause unexpected shutdowns.")
        }
        if (threads == maxThreads) {
            warnings.add("Using all CPU cores may make the device unresponsive.")
        }
        if (intensity > 80) {
            warnings.add("High mining intensity increases heat and battery drain.")
        }

        val warningMessage = if (warnings.isNotEmpty()) warnings.joinToString(" ") else null
        val riskLevel = when {
            warnings.size >= 3 -> RiskLevel.HIGH
            warnings.isNotEmpty() -> RiskLevel.MEDIUM
            else -> RiskLevel.LOW
        }

        return ValidationResult(
            isValid = true,
            warningMessage = warningMessage,
            riskLevel = riskLevel
        )
    }

    fun sanitizeInput(input: String): String {
        return input.trim()
            .replace(Regex("[<>\"';&|`]"), "")
            .replace(Regex("\\s+"), " ")
    }

    fun containsDangerousInput(input: String): Boolean {
        val lower = input.lowercase()
        return DANGEROUS_PATTERNS.any { pattern ->
            lower.contains(pattern.lowercase())
        }
    }

    fun generateConfigHash(poolAddress: String, walletAddress: String): String {
        val data = "$poolAddress|$walletAddress"
        val digest = MessageDigest.getInstance("SHA-256")
        val hashBytes = digest.digest(data.toByteArray(Charsets.UTF_8))
        return hashBytes.joinToString("") { "%02x".format(it) }
    }
}
