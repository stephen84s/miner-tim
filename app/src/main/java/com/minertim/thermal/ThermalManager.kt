package com.minertim.thermal

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.util.Log
import com.minertim.config.MiningConfig
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File

class ThermalManager(private val context: Context) {

    companion object {
        private const val TAG = "ThermalManager"
        private const val MONITORING_INTERVAL_MS = 5000L
        private const val THERMAL_ZONE_PATH = "/sys/class/thermal"
    }

    private val config = MiningConfig(context)
    private val scope = CoroutineScope(Dispatchers.IO)
    private var monitoringJob: Job? = null

    /**
     * Reads the current CPU temperature in degrees Celsius.
     * Scans all thermal_zone* directories and returns the highest temperature found.
     * Values in sysfs are in millidegrees, so we divide by 1000.
     * Returns null if no temperature could be read.
     */
    fun getCpuTemperature(): Float? {
        var maxTemp: Float? = null

        try {
            val thermalDir = File(THERMAL_ZONE_PATH)
            if (!thermalDir.exists() || !thermalDir.isDirectory) {
                Log.w(TAG, "Thermal zone directory not found: $THERMAL_ZONE_PATH")
                return null
            }

            val zones = thermalDir.listFiles { file ->
                file.isDirectory && file.name.startsWith("thermal_zone")
            } ?: return null

            for (zone in zones) {
                try {
                    val tempFile = File(zone, "temp")
                    if (!tempFile.exists() || !tempFile.canRead()) continue

                    val rawValue = tempFile.readText().trim().toLongOrNull() ?: continue
                    // Values are in millidegrees Celsius
                    val tempCelsius = rawValue / 1000.0f

                    // Sanity check: ignore clearly invalid readings
                    if (tempCelsius < 0f || tempCelsius > 150f) {
                        Log.w(TAG, "Ignoring invalid temperature reading: ${tempCelsius}°C from ${zone.name}")
                        continue
                    }

                    if (maxTemp == null || tempCelsius > maxTemp) {
                        maxTemp = tempCelsius
                    }
                } catch (e: Exception) {
                    Log.d(TAG, "Could not read temperature from ${zone.name}: ${e.message}")
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Error reading CPU temperature", e)
        }

        if (maxTemp != null) {
            Log.d(TAG, "Current CPU temperature: ${maxTemp}°C")
        }

        return maxTemp
    }

    /**
     * Returns the current battery level as a percentage (0–100),
     * or -1 if the battery level could not be determined.
     */
    fun getBatteryLevel(): Int {
        return try {
            val intentFilter = IntentFilter(Intent.ACTION_BATTERY_CHANGED)
            val batteryStatus: Intent? = context.registerReceiver(null, intentFilter)

            if (batteryStatus == null) {
                Log.w(TAG, "Could not read battery status")
                return -1
            }

            val level = batteryStatus.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val scale = batteryStatus.getIntExtra(BatteryManager.EXTRA_SCALE, -1)

            if (level >= 0 && scale > 0) {
                (level * 100) / scale
            } else {
                Log.w(TAG, "Invalid battery level values: level=$level, scale=$scale")
                -1
            }
        } catch (e: Exception) {
            Log.e(TAG, "Error reading battery level", e)
            -1
        }
    }

    /**
     * Returns true if the device is currently charging (AC or USB).
     */
    fun isCharging(): Boolean {
        return try {
            val intentFilter = IntentFilter(Intent.ACTION_BATTERY_CHANGED)
            val batteryStatus: Intent? = context.registerReceiver(null, intentFilter)

            if (batteryStatus == null) return false

            val status = batteryStatus.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
            status == BatteryManager.BATTERY_STATUS_CHARGING ||
                status == BatteryManager.BATTERY_STATUS_FULL
        } catch (e: Exception) {
            Log.e(TAG, "Error reading charging status", e)
            false
        }
    }

    /**
     * Checks whether mining can safely start based on current thermal and battery conditions.
     * Returns true if CPU temperature is below the configured maximum and
     * battery level is above the configured minimum.
     */
    fun canStartMining(): Boolean {
        val maxTemp = config.getMaxCpuTemp()
        val minBattery = config.getMinBatteryLevel()

        val currentTemp = getCpuTemperature()
        val currentBattery = getBatteryLevel()

        // If we can't read temperature, allow mining but log a warning
        if (currentTemp != null && currentTemp >= maxTemp) {
            Log.w(TAG, "CPU temperature too high: ${currentTemp}°C >= ${maxTemp}°C")
            return false
        }

        // If we can't read battery level, allow mining but log a warning
        if (currentBattery < 0) {
            Log.w(TAG, "Could not read battery level, allowing mining")
            return true
        }

        if (currentBattery < minBattery) {
            Log.w(TAG, "Battery level too low: ${currentBattery}% < ${minBattery}%")
            return false
        }

        Log.d(TAG, "Thermal check passed: temp=${currentTemp ?: "unknown"}°C, battery=${currentBattery}%")
        return true
    }

    /**
     * Starts the monitoring coroutine that checks thermal and battery conditions every 5 seconds.
     * When limits are exceeded, the [onThrottle] callback is invoked.
     */
    fun startMonitoring(onThrottle: () -> Unit) {
        if (monitoringJob?.isActive == true) {
            Log.w(TAG, "Monitoring is already active")
            return
        }

        Log.i(TAG, "Starting thermal monitoring (interval: ${MONITORING_INTERVAL_MS}ms)")

        monitoringJob = scope.launch {
            while (isActive) {
                delay(MONITORING_INTERVAL_MS)

                val maxTemp = config.getMaxCpuTemp()
                val minBattery = config.getMinBatteryLevel()

                val currentTemp = getCpuTemperature()
                val currentBattery = getBatteryLevel()

                var shouldThrottle = false

                if (currentTemp != null && currentTemp >= maxTemp) {
                    Log.w(TAG, "Thermal throttle triggered: ${currentTemp}°C >= ${maxTemp}°C")
                    shouldThrottle = true
                }

                if (currentBattery in 0 until minBattery) {
                    Log.w(TAG, "Battery throttle triggered: ${currentBattery}% < ${minBattery}%")
                    shouldThrottle = true
                }

                if (shouldThrottle) {
                    onThrottle()
                }
            }
        }
    }

    /**
     * Stops the monitoring coroutine.
     */
    fun stopMonitoring() {
        monitoringJob?.let {
            it.cancel()
            Log.i(TAG, "Thermal monitoring stopped")
        }
        monitoringJob = null
    }
}
