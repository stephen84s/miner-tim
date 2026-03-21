package com.minertim.mining

import android.app.*
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import com.minertim.R
import com.minertim.thermal.ThermalManager
import com.minertim.config.MiningConfig
import kotlinx.coroutines.*

class MiningService : Service() {
    private val binder = MiningBinder()
    private val miningCore = MiningCore()
    private var wakeLock: PowerManager.WakeLock? = null
    private var thermalManager: ThermalManager? = null
    private var miningConfig: MiningConfig? = null

    private var serviceScope = CoroutineScope(Dispatchers.Default + SupervisorJob())
    private var statsUpdateJob: Job? = null

    companion object {
        const val NOTIFICATION_ID = 1
        const val CHANNEL_ID = "mining_channel"
        const val ACTION_START_MINING = "START_MINING"
        const val ACTION_STOP_MINING = "STOP_MINING"
    }

    inner class MiningBinder : Binder() {
        fun getService(): MiningService = this@MiningService
    }

    override fun onCreate() {
        super.onCreate()

        createNotificationChannel()

        thermalManager = ThermalManager(this)
        miningConfig = MiningConfig(this)

        // Acquire wake lock to prevent device from sleeping during mining
        val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = powerManager.newWakeLock(
            PowerManager.PARTIAL_WAKE_LOCK,
            "MinerTim::MiningWakeLock"
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_MINING -> startMining()
            ACTION_STOP_MINING -> stopMining()
        }

        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        stopMining()
        serviceScope.cancel()
        wakeLock?.release()
        super.onDestroy()
    }

    fun startMining(): Boolean {
        val config = miningConfig ?: return false

        if (miningCore.isMining()) {
            return true
        }

        // Check thermal and battery conditions
        if (thermalManager?.canStartMining() != true) {
            return false
        }

        // Initialize miner
        val success = miningCore.initializeMiner(
            config.getPoolAddress(),
            config.getWalletAddress(),
            config.getThreadCount()
        )

        if (!success) {
            return false
        }

        // Start mining
        if (miningCore.startMining()) {
            wakeLock?.acquire()
            startForeground(NOTIFICATION_ID, createNotification())
            startStatsUpdates()
            thermalManager?.startMonitoring(::handleThermalThrottling)
            return true
        }

        return false
    }

    fun stopMining() {
        if (!miningCore.isMining()) {
            return
        }

        miningCore.stopMining()
        wakeLock?.release()
        stopStatsUpdates()
        thermalManager?.stopMonitoring()
        stopForeground(STOP_FOREGROUND_REMOVE)
    }

    fun isMining(): Boolean = miningCore.isMining()

    fun getHashrate(): Double = miningCore.getHashrate()

    fun getAcceptedShares(): Int = miningCore.getAcceptedShares()

    fun getRejectedShares(): Int = miningCore.getRejectedShares()

    fun setThreadCount(threads: Int) {
        miningCore.setThreadCount(threads)
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Mining Status",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Shows mining status and statistics"
                setShowBadge(false)
            }

            val notificationManager = getSystemService(NotificationManager::class.java)
            notificationManager.createNotificationChannel(channel)
        }
    }

    private fun createNotification(): Notification {
        val stopIntent = Intent(this, MiningService::class.java).apply {
            action = ACTION_STOP_MINING
        }
        val stopPendingIntent = PendingIntent.getService(
            this, 0, stopIntent, PendingIntent.FLAG_IMMUTABLE
        )

        val hashrate = String.format("%.2f", getHashrate())

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle(getString(R.string.notification_title))
            .setContentText(getString(R.string.notification_text, hashrate))
            .setSmallIcon(R.drawable.ic_mining_notification)
            .addAction(
                R.drawable.ic_stop,
                getString(R.string.stop_mining),
                stopPendingIntent
            )
            .setOngoing(true)
            .build()
    }

    private fun startStatsUpdates() {
        statsUpdateJob?.cancel()
        statsUpdateJob = serviceScope.launch {
            while (isActive && miningCore.isMining()) {
                updateNotification()
                delay(5000) // Update every 5 seconds
            }
        }
    }

    private fun stopStatsUpdates() {
        statsUpdateJob?.cancel()
        statsUpdateJob = null
    }

    private fun updateNotification() {
        val notificationManager = getSystemService(NotificationManager::class.java)
        notificationManager.notify(NOTIFICATION_ID, createNotification())
    }

    private fun handleThermalThrottling() {
        // Pause mining temporarily due to thermal throttling
        if (miningCore.isMining()) {
            miningCore.stopMining()

            // Resume mining after cooldown period
            serviceScope.launch {
                delay(30000) // Wait 30 seconds
                if (thermalManager?.canStartMining() == true) {
                    miningCore.startMining()
                }
            }
        }
    }
}
