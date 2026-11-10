/*
 * Copyright (c) 2015 Cryptonomex, Inc., and contributors.
 *
 * The MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 */
#include <assert.h>

#include <algorithm>

#include <fc/crypto/hex.hpp>
#include <fc/crypto/aes.hpp>
#include <fc/crypto/city.hpp>
#include <fc/log/logger.hpp>
#include <fc/network/ip.hpp>
#include <fc/exception/exception.hpp>

#include <graphene/net/stcp_socket.hpp>
#include <graphene/net/exceptions.hpp>

namespace graphene { namespace net {

stcp_socket::stcp_socket()
//:_buf_len(0)
#ifndef NDEBUG
   : _read_buffer_in_use(false),
     _write_buffer_in_use(false)
#endif
{
}
stcp_socket::~stcp_socket()
{
}

void stcp_socket::do_key_exchange()
{
  _priv_key = fc::ecc::private_key::generate();
  fc::ecc::public_key pub = _priv_key.get_public_key();
  fc::ecc::public_key_data s = pub.serialize();
  std::shared_ptr<char> serialized_key_buffer(new char[sizeof(fc::ecc::public_key_data)], [](char* p){ delete[] p; });
  memcpy(serialized_key_buffer.get(), (char*)&s, sizeof(fc::ecc::public_key_data));
  raw_write( serialized_key_buffer, sizeof(fc::ecc::public_key_data) );
  raw_read( serialized_key_buffer, sizeof(fc::ecc::public_key_data) );
  fc::ecc::public_key_data rpub;
  memcpy((char*)&rpub, serialized_key_buffer.get(), sizeof(fc::ecc::public_key_data));

  _shared_secret = _priv_key.get_shared_secret( rpub );
//    ilog("shared secret ${s}", ("s", shared_secret) );
  _send_aes.init( fc::sha256::hash( (char*)&_shared_secret, sizeof(_shared_secret) ), 
                  fc::city_hash_crc_128((char*)&_shared_secret,sizeof(_shared_secret) ) );
  _recv_aes.init( fc::sha256::hash( (char*)&_shared_secret, sizeof(_shared_secret) ), 
                  fc::city_hash_crc_128((char*)&_shared_secret,sizeof(_shared_secret) ) );
}


void stcp_socket::set_tls( std::shared_ptr<const tls_context> context, bool outbound_tls,
                           const std::string& server_name )
{
  if( context && context->mode() == p2p_tls_mode::off )
    context.reset();
  _tls_context = std::move(context);
  _outbound_tls = _tls_context && outbound_tls;
  _tls_server_name = server_name;
  _tls.reset();
  _unread_input.clear();
}

void stcp_socket::connect_to( const fc::ip::endpoint& remote_endpoint )
{
  _sock.connect_to( remote_endpoint );
  if( _outbound_tls )
  {
    _tls.reset( new tls_connection( *_tls_context, _sock, tls_connection::role::client, _tls_server_name ) );
    _tls->handshake();
  }
  do_key_exchange();
}

size_t stcp_socket::raw_readsome( const std::shared_ptr<char>& buffer, size_t length, size_t offset )
{
  if( _tls )
    return _tls->readsome( buffer.get() + offset, length );
  if( !_unread_input.empty() )
  {
    size_t bytes = std::min( length, _unread_input.size() );
    memcpy( buffer.get() + offset, _unread_input.data(), bytes );
    _unread_input.erase( _unread_input.begin(), _unread_input.begin() + bytes );
    return bytes;
  }
  return _sock.readsome( buffer, length, offset );
}

void stcp_socket::raw_read( const std::shared_ptr<char>& buffer, size_t length, size_t offset )
{
  while( length > 0 )
  {
    size_t bytes = raw_readsome( buffer, length, offset );
    offset += bytes;
    length -= bytes;
  }
}

void stcp_socket::raw_write( const std::shared_ptr<const char>& buffer, size_t length )
{
  if( _tls )
    _tls->write( buffer.get(), length );
  else
    _sock.write( buffer, length );
}

void stcp_socket::bind( const fc::ip::endpoint& local_endpoint )
{
  _sock.bind(local_endpoint);
}

/**
 *   This method must read at least 16 bytes at a time from
 *   the underlying TCP socket so that it can decrypt them. It
 *   will buffer any left-over.
 */
size_t stcp_socket::readsome( char* buffer, size_t len )
{ try {
    assert( len > 0 && (len % 16) == 0 );

#ifndef NDEBUG
    // This code was written with the assumption that you'd only be making one call to readsome 
    // at a time so it reuses _read_buffer.  If you really need to make concurrent calls to 
    // readsome(), you'll need to prevent reusing _read_buffer here
    struct check_buffer_in_use {
      bool& _buffer_in_use;
      check_buffer_in_use(bool& buffer_in_use) : _buffer_in_use(buffer_in_use) { assert(!_buffer_in_use); _buffer_in_use = true; }
      ~check_buffer_in_use() { assert(_buffer_in_use); _buffer_in_use = false; }
    } buffer_in_use_checker(_read_buffer_in_use);
#endif

    const size_t read_buffer_length = 4096;
    if (!_read_buffer)
      _read_buffer.reset(new char[read_buffer_length], [](char* p){ delete[] p; });

    len = std::min<size_t>(read_buffer_length, len);

    size_t s = raw_readsome( _read_buffer, len, 0 );
    if( s % 16 ) 
    {
      raw_read(_read_buffer, 16 - (s%16), s);
      s += 16-(s%16);
    }
    _recv_aes.decode( _read_buffer.get(), s, buffer );
    return s;
} FC_RETHROW_EXCEPTIONS( warn, "", ("len",len) ) }

size_t stcp_socket::readsome( const std::shared_ptr<char>& buf, size_t len, size_t offset ) 
{
  return readsome(buf.get() + offset, len);
}

bool stcp_socket::eof()const
{
  return _sock.eof();
}

size_t stcp_socket::writesome( const char* buffer, size_t len )
{ try {
    assert( len > 0 && (len % 16) == 0 );

#ifndef NDEBUG
    // This code was written with the assumption that you'd only be making one call to writesome
    // at a time so it reuses _write_buffer.  If you really need to make concurrent calls to 
    // writesome(), you'll need to prevent reusing _write_buffer here
    struct check_buffer_in_use {
      bool& _buffer_in_use;
      check_buffer_in_use(bool& buffer_in_use) : _buffer_in_use(buffer_in_use) { assert(!_buffer_in_use); _buffer_in_use = true; }
      ~check_buffer_in_use() { assert(_buffer_in_use); _buffer_in_use = false; }
    } buffer_in_use_checker(_write_buffer_in_use);
#endif

    const std::size_t write_buffer_length = 4096;
    if (!_write_buffer)
      _write_buffer.reset(new char[write_buffer_length], [](char* p){ delete[] p; });
    len = std::min<size_t>(write_buffer_length, len);
    memset(_write_buffer.get(), 0, len); // just in case aes.encode screws up
    /**
     * every sizeof(crypt_buf) bytes the aes channel
     * has an error and doesn't decrypt properly...  disable
     * for now because we are going to upgrade to something
     * better.
     */
    uint32_t ciphertext_len = _send_aes.encode( buffer, len, _write_buffer.get() );
    assert(ciphertext_len == len);
    raw_write( _write_buffer, ciphertext_len );
    return ciphertext_len;
} FC_RETHROW_EXCEPTIONS( warn, "", ("len",len) ) }

size_t stcp_socket::writesome( const std::shared_ptr<const char>& buf, size_t len, size_t offset )
{
  return writesome(buf.get() + offset, len);
}

void stcp_socket::flush()
{
  _sock.flush();
}


void stcp_socket::close()
{
  try 
  {
    _sock.close();
  }FC_RETHROW_EXCEPTIONS( warn, "error closing stcp socket" );
}

void stcp_socket::accept()
{
  if( _tls_context )
  {
    // A plain client starts with its compressed public key, a TLS client with a ClientHello.
    // Old clients write their key without waiting for ours, so reading first cannot deadlock.
    std::shared_ptr<char> first_byte( new char[1], [](char* p){ delete[] p; } );
    _sock.read( first_byte, 1 );
    const char byte = first_byte.get()[0];
    if( byte == tls_handshake_record_type )
    {
      _tls.reset( new tls_connection( *_tls_context, _sock, tls_connection::role::server ) );
      _tls->feed( &byte, 1 );
      _tls->handshake();
    }
    else if( byte == 0x02 || byte == 0x03 )
      _unread_input.push_back( byte );
    else
      FC_THROW_EXCEPTION( net_exception, "Unknown protocol: the peer started with byte ${b}",
                          ("b", fc::to_hex( &byte, 1 )) );
  }
  do_key_exchange();
}


}} // namespace graphene::net

