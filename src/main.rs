#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::can::CanConfigurator;
use embassy_stm32::peripherals::*;
use embassy_stm32::{Config, bind_interrupts, can};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use panic_probe as _;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    FDCAN1_IT0 => can::IT0InterruptHandler<FDCAN1>;
    FDCAN1_IT1 => can::IT1InterruptHandler<FDCAN1>;
});

fn rcc_init(config: &mut Config) {
    use embassy_stm32::rcc::*;
    config.rcc.hsi = Some(Hsi {
        sys_div: HsiSysDiv::DIV1,
        ker_div: HsiKerDiv::DIV1,
    });
    config.rcc.hse = None;
    config.rcc.sys = Sysclk::HSISYS;
}

fn can_init(can_cfg: &mut can::CanConfigurator) {
    // can_cfg.properties().set_extended_filter(
    //     can::filter::ExtendedFilterSlot::_0,
    //     can::filter::ExtendedFilter::accept_all_into_fifo1(),
    // );
    // 1 Mbps
    can_cfg.set_bitrate(1_000_000);
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let cfg = {
        let mut config = Config::default();
        rcc_init(&mut config);
        config
    };

    let peripherals = embassy_stm32::init(cfg);

    let mut can_cfg =
        can::CanConfigurator::new(peripherals.FDCAN1, peripherals.PD0, peripherals.PD1, Irqs);

    can_init(&mut can_cfg);
    spawner.spawn(can_controller(spawner, can_cfg).unwrap());
    // Data Processing Task
}

// ==========================================================================================================
static CAN_TX_CHANNEL: Channel<CriticalSectionRawMutex, [u8; 8], 8> = Channel::new();
static CAN_RX_CHANNEL: Channel<CriticalSectionRawMutex, [u8; 8], 8> = Channel::new();

#[embassy_executor::task]
async fn can_controller(spawner: Spawner, can_cfg: CanConfigurator<'static>) {
    info!("can_controller spawned");
    // Classic CAN
    let mut can = can_cfg.start(can::OperatingMode::NormalOperationMode);
    let (mut tx, mut rx, _props) = can.split();

    info!("CAN-figured");

    // static TX_BUF: StaticCell<can::TxBuf<8>> = StaticCell::new();
    // static RX_BUF: StaticCell<can::RxBuf<10>> = StaticCell::new();

    // Transmission Channels
    // let mut can_tx_buf = TX_BUF.init(can::TxBuf::<8>::new());
    // let mut can_rx_buf = RX_BUF.init(can::RxBuf::<10>::new());

    // Spawn Tx Rx threads
    spawner.spawn(can_tx_task(tx).unwrap());
    spawner.spawn(can_rx_task(rx).unwrap());

    // rjmp here
    loop {
        info!("controller debug");
        embassy_time::Timer::after_millis(250).await;

        let rx_data = CAN_RX_CHANNEL.receive().await;
        info!("rx thread data: {}", rx_data);
    }

    info!("can_controller destroyed");
}

#[embassy_executor::task]
async fn can_tx_task(mut tx: can::CanTx<'static>) -> ! {
    info!("can_tx_task spawned");
    loop {
        let frame = async {
            embassy_time::Timer::after_millis(250).await;
            //can::frame::Frame::new_standard(0x123u16, &[5u8]).expect("failed to create can frame")
            can::frame::Frame::new_standard(
                0x123,
                &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
            )
            .unwrap()
        }
        .await;
        info!("sending Frame");
        if let Some(queue_overflow) = tx.write(&frame).await {
            info!("buffer overflowed: {}", queue_overflow);
        }
    }
    info!("can_tx_task destroyed");
}

#[embassy_executor::task]
async fn can_rx_task(mut rx: can::CanRx<'static>) {
    info!("can_rx_task spawned");
    let mut last_read_ts = embassy_time::Instant::now();
    loop {
        // rx.read;
        // CAN_RX_CHANNEL.send(temp).await;
        // embassy_time::Timer::after_millis(250).await;
        // temp += 1;

        match rx.read().await {
            Ok(envelope) => {
                let (ts, rx_frame) = (envelope.ts, envelope.frame);
                let delta = (ts - last_read_ts).as_millis();
                last_read_ts = ts;
                info!(
                    "Rx: {} {:02x} --- {}ms",
                    rx_frame.header().len(),
                    rx_frame.data()[0..rx_frame.header().len() as usize],
                    delta,
                );

                let mut data = [0u8; 8];
                let rx_length = rx_frame.data().len();
                data[..rx_length].copy_from_slice(&rx_frame.data()[..rx_length]);

                CAN_RX_CHANNEL.send(data).await;
            }
            Err(_err) => error!("Error in frame"),
        }
    }
    info!("can_rx_task destroyed");
}
