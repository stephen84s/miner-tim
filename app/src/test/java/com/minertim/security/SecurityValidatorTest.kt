package com.minertim.security

import org.junit.Assert.*
import org.junit.Test

class SecurityValidatorTest {

    @Test
    fun `valid mainnet address is accepted`() {
        val address = "4" + "A".repeat(94)
        val result = SecurityValidator.validateMoneroAddress(address)
        assertTrue(result.isValid)
    }

    @Test
    fun `short address is rejected`() {
        val result = SecurityValidator.validateMoneroAddress("4ABC")
        assertFalse(result.isValid)
    }

    @Test
    fun `empty address is rejected`() {
        val result = SecurityValidator.validateMoneroAddress("")
        assertFalse(result.isValid)
    }

    @Test
    fun `valid pool address is accepted`() {
        val result = SecurityValidator.validatePoolAddress("pool.supportxmr.com:443")
        assertTrue(result.isValid)
    }

    @Test
    fun `pool address without port is rejected`() {
        val result = SecurityValidator.validatePoolAddress("pool.supportxmr.com")
        assertFalse(result.isValid)
    }

    @Test
    fun `whitelisted pool has low risk`() {
        val result = SecurityValidator.validatePoolAddress("pool.supportxmr.com:443")
        assertEquals(SecurityValidator.RiskLevel.LOW, result.riskLevel)
    }

    @Test
    fun `unknown pool has high risk`() {
        val result = SecurityValidator.validatePoolAddress("evil-pool.example.com:3333")
        assertTrue(result.isValid)
        assertEquals(SecurityValidator.RiskLevel.HIGH, result.riskLevel)
    }

    @Test
    fun `script injection is rejected`() {
        val sanitized = SecurityValidator.sanitizeInput("<script>alert('xss')</script>")
        assertFalse(sanitized.contains("<script"))
    }

    @Test
    fun `path traversal is rejected`() {
        val sanitized = SecurityValidator.sanitizeInput("../../../etc/passwd")
        assertFalse(sanitized.contains("../"))
    }

    @Test
    fun `valid mining config passes validation`() {
        val result = SecurityValidator.validateMiningConfig(
            threads = 2,
            maxTemp = 75.0f,
            minBattery = 20,
            intensity = 50
        )
        assertTrue(result.isValid)
    }

    @Test
    fun `zero threads fails validation`() {
        val result = SecurityValidator.validateMiningConfig(
            threads = 0,
            maxTemp = 75.0f,
            minBattery = 20,
            intensity = 50
        )
        assertFalse(result.isValid)
    }

    @Test
    fun `config hash is deterministic`() {
        val hash1 = SecurityValidator.generateConfigHash("pool:443", "4AAAA")
        val hash2 = SecurityValidator.generateConfigHash("pool:443", "4AAAA")
        assertEquals(hash1, hash2)
    }

    @Test
    fun `config hash changes with different input`() {
        val hash1 = SecurityValidator.generateConfigHash("pool:443", "4AAAA")
        val hash2 = SecurityValidator.generateConfigHash("pool:443", "4BBBB")
        assertNotEquals(hash1, hash2)
    }
}
