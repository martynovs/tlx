"""cloud.cloud_common.direct_public_ip: the security-group rule needs this machine's IPv4 address.

On a machine whose direct route prefers IPv6 the echo services answered with an IPv6 address
and every launch stopped with "cannot determine this machine's public IP" (2026-10-09, n-n15).
"""
import io
import os
import socket
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from cloud import cloud_common
import testkit


class Ipv4Only(testkit.GuardedTestCase):
    def test_connection_resolves_ipv4_only(self):
        # Known bad: an unrestricted resolver would hand back the IPv6 address first.
        seen = []

        def getaddrinfo(host, port, family=0, kind=0, *a, **k):
            seen.append(family)
            return [(socket.AF_INET, socket.SOCK_STREAM, 6, '', ('203.0.113.7', port))]

        sock = mock.Mock()
        with mock.patch.object(socket, 'getaddrinfo', getaddrinfo), \
             mock.patch.object(socket, 'socket', return_value=sock):
            self.assertIs(cloud_common.ipv4_create_connection(('example.test', 443), timeout=3), sock)
        self.assertEqual(seen, [socket.AF_INET])
        sock.connect.assert_called_once_with(('203.0.113.7', 443))
        sock.settimeout.assert_called_once_with(3)

    def test_no_ipv4_address_is_an_error_not_a_fallback(self):
        def getaddrinfo(*a, **k):
            raise socket.gaierror('no AF_INET address')
        with mock.patch.object(socket, 'getaddrinfo', getaddrinfo):
            with self.assertRaises(OSError):
                cloud_common.ipv4_create_connection(('example.test', 443))

    def test_the_https_handler_uses_the_ipv4_connection(self):
        self.assertIs(cloud_common._IPv4HTTPSConnection._create_connection,
                      cloud_common.ipv4_create_connection)


class DirectPublicIp(testkit.GuardedTestCase):
    def opener(self, answers):
        calls = []

        class Opener:
            def open(self, url, timeout=None):
                calls.append(url)
                answer = answers.get(url)
                if isinstance(answer, Exception):
                    raise answer
                return io.BytesIO(answer.encode())

        return Opener(), calls

    def test_an_ipv6_answer_moves_on_to_the_next_service(self):
        urls = cloud_common.IP_ECHO_URLS
        answers = {urls[0]: 'IP 2409:8900::1', urls[1]: '2409:8900::1', urls[2]: OSError('refused'),
                   urls[3]: '198.51.100.4\n'}
        opener, calls = self.opener(answers)
        with mock.patch.object(cloud_common.urllib.request, 'build_opener', return_value=opener):
            self.assertEqual(cloud_common.direct_public_ip(), '198.51.100.4')
        self.assertEqual(calls, list(urls))

    def test_every_service_failing_names_them_all(self):
        opener, _ = self.opener({u: OSError('down') for u in cloud_common.IP_ECHO_URLS})
        with mock.patch.object(cloud_common.urllib.request, 'build_opener', return_value=opener):
            with self.assertRaises(cloud_common.InfraError) as e:
                cloud_common.direct_public_ip()
        for url in cloud_common.IP_ECHO_URLS:
            self.assertIn(url, str(e.exception))

    def test_the_opener_has_no_proxy_and_the_ipv4_handler(self):
        built = []
        opener, _ = self.opener({cloud_common.IP_ECHO_URLS[0]: '192.0.2.9'})
        with mock.patch.object(cloud_common.urllib.request, 'build_opener',
                               side_effect=lambda *h: built.extend(h) or opener):
            cloud_common.direct_public_ip()
        self.assertTrue(any(isinstance(h, cloud_common._IPv4HTTPSHandler) for h in built))
        proxies = [h for h in built if isinstance(h, cloud_common.urllib.request.ProxyHandler)]
        self.assertEqual([h.proxies for h in proxies], [{}])


if __name__ == '__main__':
    unittest.main()
