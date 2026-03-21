package com.minertim

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import android.widget.SeekBar
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import com.minertim.config.MiningConfig
import com.minertim.databinding.ActivityMainBinding
import com.minertim.mining.MiningService
import com.minertim.security.RiskLevel
import com.minertim.security.SecurityValidator
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

class MainActivity : AppCompatActivity() {

    private lateinit var binding: ActivityMainBinding
    private lateinit var miningConfig: MiningConfig

    private var miningService: MiningService? = null
    private var serviceBound = false
    private var statsUpdateJob: Job? = null
    private var isMining = false

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as MiningService.MiningBinder
            miningService = binder.getService()
            serviceBound = true
            updateUiState()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            miningService = null
            serviceBound = false
            stopStatsUpdates()
            updateUiState()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)

        miningConfig = MiningConfig(this)

        setupUi()
        loadConfig()
    }

    override fun onStart() {
        super.onStart()
        val intent = Intent(this, MiningService::class.java)
        bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)
    }

    override fun onStop() {
        super.onStop()
        // Persist field values so they survive rotation/backgrounding
        saveFieldValues()
        stopStatsUpdates()
        if (serviceBound) {
            unbindService(serviceConnection)
            serviceBound = false
            miningService = null
        }
    }

    private fun saveFieldValues() {
        val prefs = getSharedPreferences("ui_state", Context.MODE_PRIVATE).edit()
        prefs.putString("draft_wallet", binding.etWalletAddress.text.toString())
        prefs.putString("draft_pool", binding.etPoolAddress.text.toString())
        prefs.apply()
    }

    private fun setupUi() {
        val maxThreads = Runtime.getRuntime().availableProcessors()

        // SeekBar has no min on API < 26, so we use offset: progress + 1 = threads
        binding.seekThreads.max = maxThreads - 1
        binding.seekThreads.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                val threads = progress + 1
                binding.tvThreadCount.text = getString(R.string.thread_count_label, threads)
            }
            override fun onStartTrackingTouch(seekBar: SeekBar?) {}
            override fun onStopTrackingTouch(seekBar: SeekBar?) {
                val threads = (seekBar?.progress ?: 0) + 1
                miningConfig.setThreadCount(threads)
            }
        })

        // Temperature: progress + 40 = temp (range 40-90)
        binding.seekTemperature.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                val temp = progress + 40
                binding.tvTemperature.text = getString(R.string.temperature_limit_label, temp)
            }
            override fun onStartTrackingTouch(seekBar: SeekBar?) {}
            override fun onStopTrackingTouch(seekBar: SeekBar?) {
                val temp = (seekBar?.progress ?: 35) + 40
                miningConfig.setMaxCpuTemp(temp.toFloat())
            }
        })

        // Battery: progress + 5 = level (range 5-95)
        binding.seekBattery.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                val level = progress + 5
                binding.tvBattery.text = getString(R.string.battery_minimum_label, level)
            }
            override fun onStartTrackingTouch(seekBar: SeekBar?) {}
            override fun onStopTrackingTouch(seekBar: SeekBar?) {
                val level = (seekBar?.progress ?: 15) + 5
                miningConfig.setMinBatteryLevel(level)
            }
        })

        binding.btnStartStop.setOnClickListener {
            if (isMining) {
                onStopMining()
            } else {
                onStartMining()
            }
        }
    }

    private fun loadConfig() {
        val prefs = getSharedPreferences("ui_state", Context.MODE_PRIVATE)
        val draftWallet = prefs.getString("draft_wallet", null)
        val draftPool = prefs.getString("draft_pool", null)

        binding.etWalletAddress.setText(draftWallet ?: miningConfig.getWalletAddress())
        binding.etPoolAddress.setText(draftPool ?: miningConfig.getPoolAddress())

        val threads = miningConfig.getThreadCount()
        binding.seekThreads.progress = threads - 1
        binding.tvThreadCount.text = getString(R.string.thread_count_label, threads)

        val temp = miningConfig.getMaxCpuTemp().toInt()
        binding.seekTemperature.progress = temp - 40
        binding.tvTemperature.text = getString(R.string.temperature_limit_label, temp)

        val battery = miningConfig.getMinBatteryLevel()
        binding.seekBattery.progress = battery - 5
        binding.tvBattery.text = getString(R.string.battery_minimum_label, battery)
    }

    private fun onStartMining() {
        val walletAddress = binding.etWalletAddress.text.toString().trim()
        val poolAddress = binding.etPoolAddress.text.toString().trim()

        val walletValidation = SecurityValidator.validateMoneroAddress(walletAddress)
        if (!walletValidation.isValid) {
            binding.etWalletAddress.error = walletValidation.errorMessage
            Toast.makeText(this, getString(R.string.invalid_wallet), Toast.LENGTH_LONG).show()
            return
        }

        val poolValidation = SecurityValidator.validatePoolAddress(poolAddress)
        if (!poolValidation.isValid) {
            binding.etPoolAddress.error = poolValidation.errorMessage
            Toast.makeText(this, getString(R.string.invalid_pool), Toast.LENGTH_LONG).show()
            return
        }

        if (poolValidation.riskLevel == RiskLevel.HIGH && poolValidation.warningMessage != null) {
            AlertDialog.Builder(this)
                .setTitle(getString(R.string.pool_warning))
                .setMessage(poolValidation.warningMessage)
                .setPositiveButton(android.R.string.ok) { _, _ ->
                    saveConfigAndStartMining(walletAddress, poolAddress)
                }
                .setNegativeButton(android.R.string.cancel, null)
                .show()
            return
        }

        saveConfigAndStartMining(walletAddress, poolAddress)
    }

    private fun saveConfigAndStartMining(walletAddress: String, poolAddress: String) {
        miningConfig.setWalletAddress(walletAddress)
        miningConfig.setPoolAddress(poolAddress)

        val intent = Intent(this, MiningService::class.java).apply {
            action = MiningService.ACTION_START_MINING
        }
        startService(intent)

        isMining = true
        updateUiState()
        startStatsUpdates()
    }

    private fun onStopMining() {
        val intent = Intent(this, MiningService::class.java).apply {
            action = MiningService.ACTION_STOP_MINING
        }
        startService(intent)

        isMining = false
        stopStatsUpdates()
        updateUiState()
    }

    private fun updateUiState() {
        isMining = miningService?.isMining() == true

        binding.btnStartStop.text = if (isMining) getString(R.string.stop_mining) else getString(R.string.start_mining)
        binding.tvStatus.text = if (isMining) getString(R.string.status_mining) else getString(R.string.status_idle)

        binding.etWalletAddress.isEnabled = !isMining
        binding.etPoolAddress.isEnabled = !isMining
        binding.seekThreads.isEnabled = !isMining
        binding.seekTemperature.isEnabled = !isMining
        binding.seekBattery.isEnabled = !isMining

        if (!isMining) {
            binding.tvHashrate.text = String.format(getString(R.string.hashrate_label), 0.0)
            binding.tvAcceptedShares.text = String.format(getString(R.string.accepted_shares_label), 0)
            binding.tvRejectedShares.text = String.format(getString(R.string.rejected_shares_label), 0)
        }
    }

    private fun startStatsUpdates() {
        statsUpdateJob?.cancel()
        statsUpdateJob = lifecycleScope.launch {
            while (isActive) {
                updateStats()
                delay(2000)
            }
        }
    }

    private fun stopStatsUpdates() {
        statsUpdateJob?.cancel()
        statsUpdateJob = null
    }

    private fun updateStats() {
        val service = miningService ?: return
        if (!service.isMining()) {
            stopStatsUpdates()
            updateUiState()
            return
        }

        binding.tvHashrate.text = String.format(getString(R.string.hashrate_label), service.getHashrate())
        binding.tvAcceptedShares.text = String.format(getString(R.string.accepted_shares_label), service.getAcceptedShares())
        binding.tvRejectedShares.text = String.format(getString(R.string.rejected_shares_label), service.getRejectedShares())
    }
}
