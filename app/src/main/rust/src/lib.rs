pub mod miner;
pub mod pool_connection;
pub mod randomx;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Once};

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jdouble, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::EnvUnowned;
use jni::errors::ThrowRuntimeExAndDefault;

use log::LevelFilter;

static MINER: Mutex<Option<miner::Miner>> = Mutex::new(None);
static MINING_ACTIVE: LazyLock<Arc<AtomicBool>> =
    LazyLock::new(|| Arc::new(AtomicBool::new(false)));
static INIT_LOGGER: Once = Once::new();

fn init_logger() {
    INIT_LOGGER.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(LevelFilter::Debug)
                .with_tag("MinerTim"),
        );
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_initializeMiner<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    pool: JString<'local>,
    wallet: JString<'local>,
    threads: jint,
) -> jboolean {
    init_logger();

    let result: Option<(String, String)> = env.with_env(|env| -> Result<_, jni::errors::Error> {
        let pool_str = pool.mutf8_chars(env)?.to_str().to_string();
        let wallet_str = wallet.mutf8_chars(env)?.to_str().to_string();
        Ok(Some((pool_str, wallet_str)))
    }).resolve::<ThrowRuntimeExAndDefault>();

    let (pool_str, wallet_str) = match result {
        Some(v) => v,
        None => return JNI_FALSE,
    };

    let mut miner_guard = match MINER.lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::error!("Failed to lock MINER mutex");
            return JNI_FALSE;
        }
    };

    let mut new_miner = miner::Miner::new(Arc::clone(&MINING_ACTIVE));
    match new_miner.initialize(&pool_str, &wallet_str, threads) {
        Ok(()) => {
            *miner_guard = Some(new_miner);
            log::info!("Miner initialized: pool={}, threads={}", pool_str, threads);
            JNI_TRUE
        }
        Err(e) => {
            log::error!("Failed to initialize miner: {}", e);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_startMining<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    init_logger();

    if MINING_ACTIVE.load(Ordering::SeqCst) {
        log::warn!("Mining already active");
        return;
    }

    let mut miner_guard = match MINER.lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::error!("Failed to lock MINER mutex");
            return;
        }
    };

    if let Some(ref mut m) = *miner_guard {
        MINING_ACTIVE.store(true, Ordering::SeqCst);
        if let Err(e) = m.start() {
            MINING_ACTIVE.store(false, Ordering::SeqCst);
            log::error!("Failed to start mining: {}", e);
        } else {
            log::info!("Mining started");
        }
    } else {
        log::error!("Miner not initialized");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_stopMining<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    init_logger();

    MINING_ACTIVE.store(false, Ordering::SeqCst);

    let mut miner_guard = match MINER.lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::error!("Failed to lock MINER mutex");
            return;
        }
    };

    if let Some(ref mut m) = *miner_guard {
        m.stop();
        log::info!("Mining stopped");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_getHashrate<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jdouble {
    let miner_guard = match MINER.lock() {
        Ok(guard) => guard,
        Err(_) => return 0.0,
    };

    match *miner_guard {
        Some(ref m) => m.get_hashrate(),
        None => 0.0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_getAcceptedShares<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jlong {
    let miner_guard = match MINER.lock() {
        Ok(guard) => guard,
        Err(_) => return 0,
    };

    match *miner_guard {
        Some(ref m) => m.get_accepted_shares() as jlong,
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_getRejectedShares<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jlong {
    let miner_guard = match MINER.lock() {
        Ok(guard) => guard,
        Err(_) => return 0,
    };

    match *miner_guard {
        Some(ref m) => m.get_rejected_shares() as jlong,
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_isMining<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jboolean {
    if MINING_ACTIVE.load(Ordering::SeqCst) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_setThreadCount<'local>(
    _env: EnvUnowned<'local>,
    _class: JClass<'local>,
    count: jint,
) -> jboolean {
    if MINING_ACTIVE.load(Ordering::SeqCst) {
        log::warn!("Cannot change thread count while mining");
        return JNI_FALSE;
    }

    let mut miner_guard = match MINER.lock() {
        Ok(guard) => guard,
        Err(_) => return JNI_FALSE,
    };

    if let Some(ref mut m) = *miner_guard {
        m.set_thread_count(count);
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_minertim_mining_MiningCore_stringFromJNI<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jstring {
    init_logger();

    env.with_env(|env| -> Result<_, jni::errors::Error> {
        let output = env.new_string("MinerTim Rust Engine v1.0.0")?;
        Ok(output.into_raw())
    }).resolve::<ThrowRuntimeExAndDefault>()
}
