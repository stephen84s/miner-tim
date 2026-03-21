package com.minertim.mining

class MiningCore {
    companion object {
        init {
            System.loadLibrary("minertim")
        }
    }

    external fun initializeMiner(poolAddress: String, walletAddress: String, threads: Int): Boolean
    external fun startMining(): Boolean
    external fun stopMining()
    external fun getHashrate(): Double
    external fun getAcceptedShares(): Int
    external fun getRejectedShares(): Int
    external fun isMining(): Boolean
    external fun setThreadCount(threads: Int)
    external fun stringFromJNI(): String
}
