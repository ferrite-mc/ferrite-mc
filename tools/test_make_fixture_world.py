import struct
import unittest

import make_fixture_world as fixture


def named(tag, name, payload):
    name = name.encode()
    return bytes([tag]) + struct.pack('>H', len(name)) + name + payload


def array(name, payload):
    return named(7, name, struct.pack('>i', len(payload)) + payload)


class InvalidateLightTest(unittest.TestCase):
    def test_removes_section_light_without_touching_other_data(self):
        sky = array('SkyLight', b'\xff' * 2048)
        block = array('BlockLight', b'\x11' * 2048)
        decoy = array('Other', sky + fixture.LIGHT_ON)
        section = named(1, 'Y', b'\x04') + sky + block + decoy + b'\x00'
        sections = named(9, 'sections', b'\x0a' + struct.pack('>i', 1) + section)
        raw = b'\x0a\x00\x00' + fixture.LIGHT_ON + sections + b'\x00'
        expected_section = named(1, 'Y', b'\x04') + decoy + b'\x00'
        expected = (b'\x0a\x00\x00' + fixture.LIGHT_OFF
                    + named(9, 'sections', b'\x0a' + struct.pack('>i', 1) + expected_section)
                    + b'\x00')
        self.assertEqual(fixture.invalidate_chunk_light(raw), expected)

    def test_rejects_missing_or_unlit_root_flag(self):
        with self.assertRaises(ValueError):
            fixture.invalidate_chunk_light(b'\x0a\x00\x00\x00')
        with self.assertRaises(ValueError):
            fixture.invalidate_chunk_light(b'\x0a\x00\x00' + fixture.LIGHT_OFF + b'\x00')


if __name__ == '__main__':
    unittest.main()
