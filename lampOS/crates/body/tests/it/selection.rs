use lamp_body::{
    Joint,
    command::{JointSelection, PositionFrame, SelectedPositionFrame},
    units::EncoderCounts,
};

#[test]
fn all_nonempty_subsets_have_exact_lengths_sorted_ids_and_checksum() {
    let counts = Joint::ALL.map(|j| EncoderCounts::new(i32::from(j.id()) * 733).unwrap());
    for mask in 1..=31 {
        let selection = JointSelection::from_mask(mask).unwrap();
        let frame = SelectedPositionFrame::encode(selection, counts);
        let bytes = frame.bytes();
        assert_eq!(bytes.len(), 8 + 3 * mask.count_ones() as usize);
        assert_eq!(usize::from(bytes[3]) + 4, bytes.len());
        assert_eq!(&bytes[..3], &[255, 255, 254]);
        assert_eq!(&bytes[4..7], &[0x83, 42, 2]);
        let ids: Vec<_> = bytes[7..bytes.len() - 1]
            .chunks_exact(3)
            .map(|e| {
                let j = Joint::try_from(e[0]).unwrap();
                assert_eq!(u16::from_le_bytes([e[1], e[2]]), counts[j.index()].get());
                e[0]
            })
            .collect();
        assert_eq!(
            ids,
            Joint::ALL
                .into_iter()
                .filter(|j| selection.contains(*j))
                .map(|j| j.id())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            *bytes.last().unwrap(),
            !bytes[2..bytes.len() - 1]
                .iter()
                .fold(0u8, |a, b| a.wrapping_add(*b))
        );
    }
}
#[test]
fn all_selection_preserves_existing_packet_for_every_encoder_count() {
    for count in 0..=4095 {
        let counts = [EncoderCounts::new(count).unwrap(); 5];
        assert_eq!(
            SelectedPositionFrame::encode(JointSelection::ALL, counts).bytes(),
            PositionFrame::encode(counts).bytes()
        );
    }
}
#[test]
fn empty_unknown_or_malformed_selection_never_deserializes() {
    for mask in 0..=255u8 {
        assert_eq!(
            JointSelection::from_mask(mask).is_ok(),
            (1..=31).contains(&mask)
        );
        assert_eq!(
            serde_json::from_str::<JointSelection>(&mask.to_string()).is_ok(),
            (1..=31).contains(&mask)
        );
    }
    for bad in ["-1", "256", "null", "[]", "true", "1.5", "{}"] {
        assert!(serde_json::from_str::<JointSelection>(bad).is_err());
    }
}
