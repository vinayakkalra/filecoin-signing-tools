import * as filecoin_signer_js from '@zondax/filecoin-signing-tools/js'
import filecoin_signer_wasm from '@zondax/filecoin-signing-tools'

import * as bip32Default from 'bip32'
import * as ecc from 'tiny-secp256k1';
import fs from 'fs'
import assert from 'assert'
import * as cbor from '@ipld/dag-cbor'
import secp256k1 from 'secp256k1'

import { getDigest, getDigestVoucher } from './utils.js'

const bip32 = bip32Default.BIP32Factory(ecc)

// Test twice for wasm version and pure js version
let filecoin_signer = process.env.PURE_JS ? filecoin_signer_js : filecoin_signer_wasm


/* Load wallet test data */
let rawdataWallet = fs.readFileSync('../../test_vectors/wallet.json')
let dataWallet = JSON.parse(rawdataWallet)

/* Load multisig test data */
let rawdataTxs = fs.readFileSync('../../test_vectors/payment_channel.json')
let dataTxs = JSON.parse(rawdataTxs)

/* Load voucher test data */
let rawdataVoucher = fs.readFileSync('../../test_vectors/voucher.json')
let dataVoucher = JSON.parse(rawdataVoucher)

const MASTER_NODE = bip32.fromBase58(dataWallet.master_key)

let describeCall = describe
if (process.env.PURE_JS) {
  describeCall = describe.skip
}

describeCall('createPymtChan', function() {
  it('create payment channel transaction and sign (SECP256K1)', function() {
    let paymentchannel_create = dataTxs.creation.secp256k1
    let recoveredKey = filecoin_signer.keyRecover(paymentchannel_create.private_key, true)

    let constructor_params = { 
      From: paymentchannel_create.constructor_params["From"],
      To: paymentchannel_create.constructor_params["To"],
    }

    let params = {
      CodeCid: 'bafk2bzacebalad3f72wyk7qyilvfjijcwubdspytnyzlrhvn73254gqis44rq',
      ConstructorParams: Buffer.from(filecoin_signer.serializeParams(constructor_params)).toString('base64')
    }

    let serialized_params = filecoin_signer.serializeParams(params);

    let create_pymtchan = {
      To: paymentchannel_create.message["To"],
      From: paymentchannel_create.message["From"],
      Nonce: paymentchannel_create.message["Nonce"],
      Value: paymentchannel_create.message["Value"],
      GasLimit: paymentchannel_create.message["GasLimit"],
      GasFeeCap: paymentchannel_create.message["GasFeeCap"],
      GasPremium: paymentchannel_create.message["GasPremium"],
      Method: paymentchannel_create.message["Method"],
      Params: Buffer.from(serialized_params).toString('base64')
    }


    /*let create_pymtchan = filecoin_signer.createPymtChanWithFee(
      paymentchannel_create.constructor_params["From"],
      paymentchannel_create.constructor_params["To"],
      paymentchannel_create.message["Value"],
      paymentchannel_create.message["Nonce"],
      paymentchannel_create.message["GasLimit"].toString(),
      paymentchannel_create.message["GasFeeCap"],
      paymentchannel_create.message["GasPremium"],
      "mainnet"
    )*/

    let signedMessage = filecoin_signer.transactionSignLotus(create_pymtchan, paymentchannel_create.private_key)
    signedMessage = JSON.parse(signedMessage)

    assert.deepStrictEqual(paymentchannel_create.message.params, create_pymtchan.params)

    const signature = Buffer.from(signedMessage.Signature.Data, 'base64')

    const serializedMessage = filecoin_signer.transactionSerialize(create_pymtchan)

    const messageDigest = getDigest(Buffer.from(serializedMessage, 'hex'))

    // Remove the V value from the signature (last byte)
    assert(secp256k1.ecdsaVerify(signature.slice(0, -1), messageDigest, recoveredKey.public_raw))

  })
  it('create payment channel transaction and sign (BLS)', function() {
    let paymentchannel_create = dataTxs.creation.bls

    let constructor_params = { 
      From: paymentchannel_create.constructor_params["From"],
      To: paymentchannel_create.constructor_params["To"],
    }

    let params = {
      CodeCid: 'bafk2bzacebalad3f72wyk7qyilvfjijcwubdspytnyzlrhvn73254gqis44rq',
      ConstructorParams: Buffer.from(filecoin_signer.serializeParams(constructor_params)).toString('base64')
    }

    let serialized_params = filecoin_signer.serializeParams(params);

    let create_pymtchan = {
      To: paymentchannel_create.message["To"],
      From: paymentchannel_create.message["From"],
      Nonce: paymentchannel_create.message["Nonce"],
      Value: paymentchannel_create.message["Value"],
      GasLimit: paymentchannel_create.message["GasLimit"],
      GasFeeCap: paymentchannel_create.message["GasFeeCap"],
      GasPremium: paymentchannel_create.message["GasPremium"],
      Method: paymentchannel_create.message["Method"],
      Params: Buffer.from(serialized_params).toString('base64')
    }

    /*let create_pymtchan = filecoin_signer.createPymtChanWithFee(
      paymentchannel_create.constructor_params["From"],
      paymentchannel_create.constructor_params["To"],
      paymentchannel_create.message["Value"],
      paymentchannel_create.message["Nonce"],
      paymentchannel_create.message["GasLimit"].toString(),
      paymentchannel_create.message["GasFeeCap"],
      paymentchannel_create.message["GasPremium"],
      "mainnet"
    )*/

    let signedMessage = filecoin_signer.transactionSignLotus(create_pymtchan, paymentchannel_create.private_key)
    signedMessage = JSON.parse(signedMessage)

    assert.deepStrictEqual(paymentchannel_create.message["Params"], create_pymtchan["Params"])

    // TODO: verify signature
    // but with which lib ?

  })
})

describeCall('updatePymtChan', function() {
  it('update payment channel transaction and sign', function() {
    let paymentchannel_update = dataTxs.update.secp256k1
    let recoveredKey = filecoin_signer.keyRecover(paymentchannel_update.private_key, true)

    let voucher = filecoin_signer.deserializeVoucher(paymentchannel_update.voucher_base64)

    let params = { 
      Sv: voucher,
      Secret: "",
    }

    let serialized_params = filecoin_signer.serializeParams(params)

    let update_pymtchan = {
      To: paymentchannel_update.message["To"],
      From: paymentchannel_update.message["From"],
      Nonce: paymentchannel_update.message["Nonce"],
      Value: paymentchannel_update.message["Value"],
      GasLimit: paymentchannel_update.message["GasLimit"],
      GasFeeCap: paymentchannel_update.message["GasFeeCap"],
      GasPremium: paymentchannel_update.message["GasPremium"],
      Method: paymentchannel_update.message["Method"],
      Params: Buffer.from(serialized_params).toString('base64')
    }

    /*let update_pymtchan = filecoin_signer.updatePymtChanWithFee(
      paymentchannel_update.message["To"],
      paymentchannel_update.message["From"],
      paymentchannel_update.voucher_base64,
      paymentchannel_update.message["Nonce"],
      paymentchannel_update.message["GasLimit"].toString(),
      paymentchannel_update.message["GasFeeCap"],
      paymentchannel_update.message["GasPremium"],
    )*/

    let signedMessage = filecoin_signer.transactionSignLotus(update_pymtchan, paymentchannel_update.private_key)
    signedMessage = JSON.parse(signedMessage)

    assert.deepStrictEqual(paymentchannel_update.message, update_pymtchan)

    const signature = Buffer.from(signedMessage.Signature.Data, 'base64')

    const serializedMessage = filecoin_signer.transactionSerialize(update_pymtchan)

    const messageDigest = getDigest(Buffer.from(serializedMessage, 'hex'))

    // Remove the V value from the signature (last byte)
    assert(secp256k1.ecdsaVerify(signature.slice(0, -1), messageDigest, recoveredKey.public_raw))
  })
})

describeCall('settlePymtChan', function() {
  it('settle payment channel and sign', function() {
    let paymentchannel_settle = dataTxs.settle.secp256k1
    let recoveredKey = filecoin_signer.keyRecover(paymentchannel_settle.private_key, true)

    let settle_pymtchan = {
      To: paymentchannel_settle.message["To"],
      From: paymentchannel_settle.message["From"],
      Nonce: paymentchannel_settle.message["Nonce"],
      Value: paymentchannel_settle.message["Value"],
      GasLimit: paymentchannel_settle.message["GasLimit"],
      GasFeeCap: paymentchannel_settle.message["GasFeeCap"],
      GasPremium: paymentchannel_settle.message["GasPremium"],
      Method: paymentchannel_settle.message["Method"],
      Params: ''
    }


    /*let settle_pymtchan = filecoin_signer.settlePymtChanWithFee(
      paymentchannel_settle.message["To"],
      paymentchannel_settle.message["From"],
      paymentchannel_settle.message["Nonce"],
      paymentchannel_settle.message["GasLimit"].toString(),
      paymentchannel_settle.message["GasFeeCap"],
      paymentchannel_settle.message["GasPremium"],
    )*/

    let signedMessage = filecoin_signer.transactionSignLotus(settle_pymtchan, paymentchannel_settle.private_key)
    signedMessage = JSON.parse(signedMessage)

    assert.deepStrictEqual(paymentchannel_settle.message, settle_pymtchan)

    const signature = Buffer.from(signedMessage.Signature.Data, 'base64')
    const serializedMessage = filecoin_signer.transactionSerialize(settle_pymtchan)
    const messageDigest = getDigest(Buffer.from(serializedMessage, 'hex'))

    // Remove the V value from the signature (last byte)
    assert(secp256k1.ecdsaVerify(signature.slice(0, -1), messageDigest, recoveredKey.public_raw))
  })
})

describeCall('collectPymtChan', function() {
  it('collect payment channel and sign', function() {
    let paymentchannel_collect = dataTxs.collect.secp256k1
    let recoveredKey = filecoin_signer.keyRecover(paymentchannel_collect.private_key, true)

    let collect_pymtchan = {
      To: paymentchannel_collect.message["To"],
      From: paymentchannel_collect.message["From"],
      Nonce: paymentchannel_collect.message["Nonce"],
      Value: paymentchannel_collect.message["Value"],
      GasLimit: paymentchannel_collect.message["GasLimit"],
      GasFeeCap: paymentchannel_collect.message["GasFeeCap"],
      GasPremium: paymentchannel_collect.message["GasPremium"],
      Method: paymentchannel_collect.message["Method"],
      Params: ''
    }

    /*let collect_pymtchan = filecoin_signer.collectPymtChanWithFee(
      paymentchannel_collect.message["To"],
      paymentchannel_collect.message["From"],
      paymentchannel_collect.message["Nonce"],
      paymentchannel_collect.message["GasLimit"].toString(),
      paymentchannel_collect.message["GasFeeCap"],
      paymentchannel_collect.message["GasPremium"],
    )*/

    let signedMessage = filecoin_signer.transactionSignLotus(collect_pymtchan, paymentchannel_collect.private_key)
    signedMessage = JSON.parse(signedMessage)

    assert.deepStrictEqual(paymentchannel_collect.message, collect_pymtchan)

    const signature = Buffer.from(signedMessage.Signature.Data, 'base64')
    const serializedMessage = filecoin_signer.transactionSerialize(collect_pymtchan)
    const messageDigest = getDigest(Buffer.from(serializedMessage, 'hex'))

    // Remove the V value from the signature (last byte)
    assert(secp256k1.ecdsaVerify(signature.slice(0, -1), messageDigest, recoveredKey.public_raw))
  })
})

describeCall('createVoucher', function() {
  it('create a voucher', function() {
    let voucher_expected = dataVoucher.sign.voucher

    const voucher = filecoin_signer.createVoucher(
      voucher_expected.payment_channel_address,
      voucher_expected.time_lock_min.toString(),
      voucher_expected.time_lock_max.toString(),
      voucher_expected.amount,
      voucher_expected.lane.toString(),
      voucher_expected.nonce,
      voucher_expected.min_settle_height.toString(),
    )

    assert(voucher)
  })
})

describeCall('signVoucher', function() {
  it('sign a voucher', function() {
    let voucher_expected = dataVoucher.sign.voucher

    let child = MASTER_NODE.derivePath('44\'/1\'/0/0/0')
    let privateKey = child.privateKey.toString('base64')

    let recoveredKey = filecoin_signer.keyRecover(privateKey, true)

    const voucher = filecoin_signer.createVoucher(
      voucher_expected.payment_channel_address,
      voucher_expected.time_lock_min.toString(),
      voucher_expected.time_lock_max.toString(),
      voucher_expected.amount,
      voucher_expected.lane.toString(),
      voucher_expected.nonce,
      voucher_expected.min_settle_height.toString(),
    )

    const signedVoucher = filecoin_signer.signVoucher(voucher, privateKey)

    let signature = cbor.decode(Buffer.from(signedVoucher, 'base64'))[10]

    signature = signature.slice(1, -1)

    const messageDigest = getDigestVoucher(Buffer.from(voucher, 'base64'))

    assert(secp256k1.ecdsaVerify(signature, messageDigest, recoveredKey.public_raw))
  })

})

describeCall('verifyVoucherSignature', function() {
  it('should return true', function() {
    let voucher = dataVoucher.verify

    assert(filecoin_signer.verifyVoucherSignature(voucher.signed_voucher_base64, voucher.address_signer))
  })
})
